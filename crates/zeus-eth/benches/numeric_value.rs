use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use zeus_eth::alloy_primitives::U256;
use zeus_eth::utils::NumericValue;

/// Typical USD prices / balances shown on the portfolio row path.
const TYPICAL_PRICES: [f64; 8] = [
   4304.34,
   1.0,
   0.9998,
   12.47,
   0.01,
   0.001,
   0.000001834247995202872,
   100_000.0,
];
const TYPICAL_BALANCES: [f64; 8] = [
   1.25,
   250.0,
   0.003009581964807856,
   50.0,
   1e9,
   0.0,
   12.345,
   0.01,
];

fn bench_currency_price(c: &mut Criterion) {
   let mut group = c.benchmark_group("currency_price");
   group.bench_function("typical_eth", |b| {
      b.iter(|| NumericValue::currency_price(black_box(4304.34)))
   });
   group.bench_function("tiny", |b| {
      b.iter(|| NumericValue::currency_price(black_box(0.000001834247995202872)))
   });
   group.bench_function("comma", |b| {
      b.iter(|| NumericValue::currency_price(black_box(100_000.00)))
   });
   group.bench_function("abbreviated", |b| {
      b.iter(|| NumericValue::currency_price(black_box(725_230_000.00)))
   });
   group.finish();
}

fn bench_value(c: &mut Criterion) {
   c.bench_function("value_amount_times_price", |b| {
      b.iter(|| NumericValue::value(black_box(1.25), black_box(4304.34)))
   });
}

fn bench_format_wei(c: &mut Criterion) {
   let one_eth = U256::from(1_000_000_000_000_000_000u128);
   let dust = U256::from(3_009_581_964_807_856u128);
   let mut group = c.benchmark_group("format_wei");
   group.bench_function("one_eth", |b| {
      b.iter(|| NumericValue::format_wei(black_box(one_eth), black_box(18)))
   });
   group.bench_function("dust", |b| {
      b.iter(|| NumericValue::format_wei(black_box(dust), black_box(18)))
   });
   group.finish();
}

fn bench_portfolio_row_uncached(c: &mut Criterion) {
   c.bench_function("portfolio_8_rows_price_and_value", |b| {
      b.iter(|| {
         let mut n = 0usize;
         for i in 0..TYPICAL_PRICES.len() {
            let price = NumericValue::currency_price(black_box(TYPICAL_PRICES[i]));
            let value = NumericValue::value(
               black_box(TYPICAL_BALANCES[i]),
               black_box(TYPICAL_PRICES[i]),
            );
            n += price.abbreviated().len() + value.abbreviated().len();
         }
         black_box(n)
      })
   });
}

criterion_group!(
   benches,
   bench_currency_price,
   bench_value,
   bench_format_wei,
   bench_portfolio_row_uncached
);
criterion_main!(benches);
