use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::Registry;

use tracing_subscriber::{
   EnvFilter, fmt, layer::SubscriberExt, prelude::*, util::SubscriberInitExt,
};

pub fn setup_tracing() -> (WorkerGuard, WorkerGuard) {
   // Setup for file appenders
   let trace_appender = tracing_appender::rolling::daily("./logs", "trace.log");
   let output_appender = tracing_appender::rolling::daily("./logs", "output.log");

   // Creating non-blocking writers
   let (trace_writer, trace_guard) = tracing_appender::non_blocking(trace_appender);
   let (output_writer, output_guard) = tracing_appender::non_blocking(output_appender);

   // Use different filters for trace logs and other levels
   let console_filter = EnvFilter::new("zeus_desktop=info,error,warn,zeus_eth=info,error,warn");
   let trace_filter = EnvFilter::new("zeus_desktop=trace,zeus_eth=trace");
   let output_filter = EnvFilter::new("zeus_desktop=info,error,warn,zeus_eth=info,error,warn");

   // Setting up layers
   let console_layer = fmt::layer()
      .with_writer(std::io::stdout)
      .with_filter(console_filter);

   let trace_layer = fmt::layer()
      .with_writer(trace_writer)
      .with_filter(trace_filter);

   let output_layer = fmt::layer()
      .with_writer(output_writer)
      .with_filter(output_filter);

   // Applying configuration
   Registry::default()
      .with(trace_layer)
      .with(console_layer)
      .with(output_layer)
      .init();

   (trace_guard, output_guard)
}
