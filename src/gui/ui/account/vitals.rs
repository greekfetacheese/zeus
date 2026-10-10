//! The machine Zeus is running on: the hardening of its own memory, and the vitals of
//! the box underneath it.
//!
//! Everything is read on a worker thread (see [`SystemVitals::refresh_if_stale`]) and
//! stored already formatted, so laying out the Diagnostics tab never measures anything.

use super::PANEL_ROW_WIDTH;
use crate::gui::SHARED_GUI;
use crate::gui::dots_button;
use crate::utils::RT;
use egui::{Align, Layout, Margin, RichText, TextWrapMode, Ui, vec2};
use egui_elements::{Label, Theme};
use elegance::{Indicator, IndicatorState, Menu, MenuItem};
use std::time::{Duration, Instant};
use sysinfo::{
   CpuRefreshKind, MINIMUM_CPU_UPDATE_INTERVAL, ProcessesToUpdate, System, get_current_pid,
};

/// How long a snapshot stays fresh before the panel asks for another one. The numbers
/// move slowly; polling harder would only cost wakeups.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// Diameter of an elegance `Indicator` at its default size, so the row that shows one
/// insets its label by a known amount.
const INDICATOR_SIZE: f32 = 10.0;

/// `RLIMIT_MEMLOCK`: the soft limit this process runs under (`ulimit -l`) and the
/// explanation for its hover.
pub struct Memlock {
   /// Soft limit, as the value column shows it (`unlimited` included).
   pub soft: String,
   /// What the limit is, with the hard ceiling it may be raised to.
   pub hover: String,
}

/// Snapshot of the local machine and of Zeus's own process.
///
/// Every visible field is already formatted, so the frame path only reads them.
#[derive(Default)]
pub struct SystemVitals {
   /// `memfd_secret`-backed allocations are available (Linux only).
   pub memfd: bool,
   /// `RLIMIT_MEMLOCK`, where the platform has one.
   pub memlock: Option<Memlock>,
   pub os: String,
   pub kernel: String,
   pub cpu: String,
   /// Detail behind the CPU line (architecture, core and thread counts).
   pub cpu_hover: String,
   pub load: String,
   pub memory: String,
   pub uptime: String,
   /// Zeus's own process: pid, private resident memory, time since it started.
   pub process: String,
   /// What `process` measures, and the resident total it was read against.
   pub process_hover: String,
   /// Staleness bookkeeping, not part of the readout.
   refreshed_at: Option<Instant>,
   refreshing: bool,
}

impl SystemVitals {
   /// Asks for a fresh snapshot once the current one has gone stale. All of the reading
   /// happens on a worker; the frame path only sees the result arrive.
   pub fn refresh_if_stale(&mut self) {
      let fresh = self.refreshed_at.is_some_and(|at| at.elapsed() < REFRESH_INTERVAL);

      if self.refreshing || fresh {
         return;
      }

      self.refreshing = true;

      RT.spawn_blocking(|| {
         let vitals = Self::read();

         SHARED_GUI.write(|gui| {
            gui.account_panel.vitals = vitals;
            gui.request_repaint();
         });
      });
   }

   /// The whole readout as plain text, for the clipboard.
   pub fn report(&self) -> String {
      let memfd = match self.memfd {
         true => "supported",
         false => "unavailable",
      };

      let mut report = format!(
         "Zeus {} — local vitals\n",
         env!("CARGO_PKG_VERSION")
      );
      report.push_str(&report_line("memfd_secret", memfd));

      if let Some(memlock) = &self.memlock {
         report.push_str(&report_line("RLIMIT_MEMLOCK", &memlock.soft));
      }

      report.push_str(&report_line("OS", &self.os));

      if !self.kernel.is_empty() {
         report.push_str(&report_line("Kernel", &self.kernel));
      }

      report.push_str(&report_line("CPU", &self.cpu));
      report.push_str(&report_line("Load", &self.load));
      report.push_str(&report_line("Memory", &self.memory));
      report.push_str(&report_line("Uptime", &self.uptime));
      report.push_str(&report_line("Zeus", &self.process));
      report
   }

   /// Reads the whole snapshot. Blocking: the CPU figure needs two samples at least
   /// [`MINIMUM_CPU_UPDATE_INTERVAL`] apart, so this sleeps between them.
   fn read() -> Self {
      let mut sys = System::new();
      sys.refresh_cpu_list(CpuRefreshKind::nothing());
      sys.refresh_cpu_usage();
      std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
      sys.refresh_cpu_usage();
      sys.refresh_memory();

      let threads = sys.cpus().len();
      let arch = System::cpu_arch();
      let brand = sys
         .cpus()
         .first()
         .map(|cpu| cpu.brand().trim())
         .filter(|brand| !brand.is_empty());

      let cpu = match brand {
         Some(brand) => format!("{brand} ×{threads}"),
         None => format!("{arch} ×{threads}"),
      };

      let cores = System::physical_core_count()
         .map(|cores| format!("{cores} cores, "))
         .unwrap_or_default();
      let cpu_hover = format!("{arch} · {cores}{threads} threads");

      let total = sys.total_memory();
      let used = total.saturating_sub(sys.available_memory());
      let memory = match total {
         0 => format!("{} used", format_bytes(used)),
         _ => format!(
            "{} / {} ({}%)",
            format_bytes(used),
            format_bytes(total),
            used * 100 / total
         ),
      };

      let mut process = "unavailable".to_string();
      let mut process_hover = "Process statistics are not available on this platform.".to_string();

      if let Ok(pid) = get_current_pid() {
         sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);

         if let Some(me) = sys.process(pid) {
            let resident = me.memory();
            let private = private_memory();
            let run_time = format_duration(me.run_time());

            process = format!(
               "pid {} · {} · {run_time}",
               pid.as_u32(),
               format_bytes(private.unwrap_or(resident))
            );

            process_hover = match private.is_some() {
               true => format!(
                  "Private resident memory: what terminating Zeus would hand back, shared \
                   pages excluded. Resident set size including them: {}.",
                  format_bytes(resident)
               ),
               false => "Resident set size. This platform does not report Zeus's private \
                        pages, so shared ones are included."
                  .to_string(),
            };
         }
      }

      Self {
         memfd: memfd_supported(),
         memlock: memlock_limits(),
         os: System::long_os_version()
            .or_else(System::name)
            .unwrap_or_else(|| "unknown".to_string()),
         kernel: System::kernel_version().unwrap_or_default(),
         cpu,
         cpu_hover,
         load: format!("{:.1}%", sys.global_cpu_usage()),
         memory,
         uptime: format_duration(System::uptime()),
         process,
         process_hover,
         refreshed_at: Some(Instant::now()),
         refreshing: false,
      }
   }
}

/// The Diagnostics card: how Zeus's own memory is protected, then the machine's vitals.
///
/// Width matches the status frames above it — `PANEL_ROW_WIDTH` painted. `set_width`
/// sizes the *content* box and the frame paints its margin outside it, so the content
/// asks for the constant minus that margin.
pub fn card(theme: &Theme, ui: &mut Ui, vitals: &SystemVitals) {
   let frame = theme.frame1.inner_margin(Margin::same(5));

   frame.show(ui, |ui| {
      ui.set_max_width(PANEL_ROW_WIDTH - frame.inner_margin.sum().x);
      // Zero row spacing: the readout is a block.
      ui.spacing_mut().item_spacing.y = 0.0;

      heading(theme, ui, vitals);

      let (state, value) = match vitals.memfd {
         true => (IndicatorState::On, "supported"),
         false => (IndicatorState::Off, "unavailable"),
      };

      Row::new("Memfd secret", value)
         .indicator(state)
         .label_hover(
            "memfd_secret backed allocations, Linux only. Other platforms fall back to malloc.",
         )
         .show(theme, ui);

      if let Some(memlock) = &vitals.memlock {
         Row::new("RLIMIT_MEMLOCK", &memlock.soft)
            .value_hover(&memlock.hover)
            .show(theme, ui);
      }

      // The OS string usually carries the kernel; offer it only when it does not.
      let kernel_hover = (!vitals.kernel.is_empty() && !vitals.os.contains(&vitals.kernel))
         .then_some(vitals.kernel.as_str());

      let mut os = Row::new("OS", &vitals.os);
      if let Some(hover) = kernel_hover {
         os = os.value_hover(hover);
      }
      os.show(theme, ui);

      Row::new("CPU", &vitals.cpu).value_hover(&vitals.cpu_hover).show(theme, ui);

      Row::new("Load now", &vitals.load)
         .value_hover("Average load across all cores, sampled over the last couple of seconds.")
         .show(theme, ui);

      Row::new("Memory", &vitals.memory)
         .value_hover("Used / total physical memory.")
         .show(theme, ui);

      Row::new("Zeus", &vitals.process)
         .value_hover(&vitals.process_hover)
         .show(theme, ui);
   });
}

/// The card's title, the machine's uptime, and the `Copy report` menu.
fn heading(theme: &Theme, ui: &mut Ui, vitals: &SystemVitals) {
   ui.horizontal(|ui| {
      let title = RichText::new("VITALS")
         .size(theme.typography.small)
         .color(theme.colors.text_muted);
      ui.label(title);

      ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.sm;

         let size = vec2(24.0, 12.0);
         let more = dots_button(theme, size, ui);

         Menu::new(("vitals_menu", "local_vitals")).show_below(&more, |ui| {
            if ui.add(MenuItem::new("Copy report")).clicked() {
               ui.ctx().copy_text(vitals.report());
            }
         });

         // The machine's uptime rides here rather than in a row of its own: the card has
         // a fixed budget to respect, and this line costs the heading nothing.
         if !vitals.uptime.is_empty() {
            let uptime = RichText::new(format!("up {}", vitals.uptime))
               .size(theme.typography.small)
               .color(theme.colors.text_muted);
            ui.label(uptime).on_hover_text("How long this machine has been up.");
         }
      });
   });
}

/// One line of the readout: an optional indicator, the label, and the value
/// right-anchored so the column stays put whatever either side measures.
struct Row<'a> {
   indicator: Option<IndicatorState>,
   label: &'a str,
   value: &'a str,
   label_hover: Option<&'a str>,
   value_hover: Option<&'a str>,
}

impl<'a> Row<'a> {
   fn new(label: &'a str, value: &'a str) -> Self {
      Self {
         indicator: None,
         label,
         value,
         label_hover: None,
         value_hover: None,
      }
   }

   fn indicator(mut self, state: IndicatorState) -> Self {
      self.indicator = Some(state);
      self
   }

   fn label_hover(mut self, hover: &'a str) -> Self {
      self.label_hover = Some(hover);
      self
   }

   fn value_hover(mut self, hover: &'a str) -> Self {
      self.value_hover = Some(hover);
      self
   }

   fn show(&self, theme: &Theme, ui: &mut Ui) {
      ui.horizontal(|ui| {
         // No slot is held for a missing dot: only the row that has one is inset, so
         // every other label starts at the card's edge instead of in an empty gutter.
         if let Some(state) = self.indicator {
            ui.add(Indicator::new(state).size(INDICATOR_SIZE));
            ui.add_space(theme.spacing.sm);
         }

         let label = RichText::new(self.label).size(theme.typography.very_small);
         let label = ui.add(Label::new(label, None));

         if let Some(hover) = self.label_hover {
            label.on_hover_text(hover);
         }

         ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let value = RichText::new(self.value).size(theme.typography.very_small);

            let value = ui.add(Label::new(value, None).wrap_mode(TextWrapMode::Truncate));

            if let Some(hover) = self.value_hover {
               value.on_hover_text(hover);
            }
         });
      });
   }
}

/// One `label  value` line of the clipped report, labels padded so the values line up.
fn report_line(label: &str, value: &str) -> String {
   format!("{label:<16} {value}\n")
}

/// A byte count in `KiB`/`MiB`/`GiB`, switching unit at 1024 so a limit the kernel set
/// to 8 MiB reads back as `8.00 MiB`.
fn format_bytes(bytes: u64) -> String {
   const KIB: f64 = 1024.0;

   let kib = bytes as f64 / KIB;
   match kib {
      k if k < KIB => format!("{kib:.2} KiB"),
      k if k < KIB * KIB => format!("{:.2} MiB", k / KIB),
      k => format!("{:.1} GiB", k / (KIB * KIB)),
   }
}

/// A duration at the size these values deserve: `12s`, `04:12`, `01:04:12`, `3d 04:12`.
fn format_duration(seconds: u64) -> String {
   let (days, rest) = (seconds / 86_400, seconds % 86_400);
   let (hours, rest) = (rest / 3600, rest % 3600);
   let (minutes, seconds) = (rest / 60, rest % 60);

   match (days, hours) {
      (0, 0) if minutes == 0 => format!("{seconds}s"),
      (0, 0) => format!("{minutes:02}:{seconds:02}"),
      (0, _) => format!("{hours:02}:{minutes:02}:{seconds:02}"),
      (days, _) => format!("{days}d {hours:02}:{minutes:02}"),
   }
}

/// Resident memory the kernel attributes to this process and nothing else — what
/// terminating it would actually hand back. `/proc/self/smaps_rollup` is the kernel's own
/// aggregate of the per-mapping `smaps` entries, so this is one small read rather than a
/// full page-table walk. `None` where the file is absent or the platform has no such
/// notion, in which case the caller falls back to the resident set size.
#[cfg(target_os = "linux")]
fn private_memory() -> Option<u64> {
   let rollup = std::fs::read_to_string("/proc/self/smaps_rollup").ok()?;
   let mut kib = 0u64;

   for line in rollup.lines() {
      let Some((key, rest)) = line.split_once(':') else {
         continue;
      };

      let Some(value) = rest.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()) else {
         continue;
      };

      // Private_Clean + Private_Dirty are the pages no other process maps; Private_Hugetlb
      // is the same accounting for huge pages and belongs with them.
      if matches!(
         key,
         "Private_Clean" | "Private_Dirty" | "Private_Hugetlb"
      ) {
         kib = kib.saturating_add(value);
      }
   }

   // A live process always owns private pages, so zero means the parse found nothing.
   (kib > 0).then(|| kib * 1024)
}

#[cfg(not(target_os = "linux"))]
fn private_memory() -> Option<u64> {
   None
}

/// `memfd_secret` support. Linux-only in the kernel, so every other platform (and every
/// other unix) reports `false` — which is what the allocator is actually using there.
fn memfd_supported() -> bool {
   #[cfg(unix)]
   {
      secure_types::supports_memfd_secret()
   }
   #[cfg(not(unix))]
   {
      false
   }
}

/// `RLIMIT_MEMLOCK` for this process. Linux-only: other platforms define the constant
/// without enforcing it, so nothing is reported there.
#[cfg(target_os = "linux")]
fn memlock_limits() -> Option<Memlock> {
   let mut limit = libc::rlimit {
      rlim_cur: 0,
      rlim_max: 0,
   };

   // SAFETY: `getrlimit` only writes the two fields of the struct handed to it.
   if unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut limit) } != 0 {
      return None;
   }

   let infinity = libc::RLIM_INFINITY as u64;
   let soft = format_limit(limit.rlim_cur as u64, infinity);
   let hard = format_limit(limit.rlim_max as u64, infinity);

   Some(Memlock {
      hover: format!("Soft limit on memory this process may lock (ulimit -l). Hard limit: {hard}"),
      soft,
   })
}

#[cfg(not(target_os = "linux"))]
fn memlock_limits() -> Option<Memlock> {
   None
}

/// An `rlimit` value: `unlimited` for the platform's infinity, else the byte count.
#[cfg(target_os = "linux")]
fn format_limit(value: u64, infinity: u64) -> String {
   match value == infinity {
      true => "unlimited".to_string(),
      false => format_bytes(value),
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn formats_bytes_in_binary_units() {
      assert_eq!(format_bytes(0), "0.00 KiB");
      assert_eq!(format_bytes(1024), "1.00 KiB");
      assert_eq!(format_bytes(65_536), "64.00 KiB");
      assert_eq!(format_bytes(8_388_608), "8.00 MiB");
      assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
   }

   #[test]
   fn formats_durations() {
      assert_eq!(format_duration(0), "0s");
      assert_eq!(format_duration(45), "45s");
      assert_eq!(format_duration(252), "04:12");
      assert_eq!(format_duration(3723), "01:02:03");
      assert_eq!(format_duration(273_852), "3d 04:04");
   }

   #[test]
   fn rlimit_infinity_reads_as_unlimited() {
      assert_eq!(format_limit(u64::MAX, u64::MAX), "unlimited");
      assert_eq!(format_limit(8_388_608, u64::MAX), "8.00 MiB");
   }
}
