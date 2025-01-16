// Credits: https://github.com/normdoow/uniswap.fish

use alloy_primitives::U256;
use bigdecimal::{BigDecimal, FromPrimitive};
use uniswap_v3_math::sqrt_price_math::Q96;
use std::str::FromStr;

use super::PoolTick;


#[derive(Debug, Clone)]
pub struct DepositAmounts {
    /// Amount of token0 to deposit
    pub amount0: f64,

    /// Amount of token1 to deposit
    pub amount1: f64,

    /// Liquidity delta
    pub liquidity_delta: f64,
}

/// Estimate the earned fees in USD value
///
/// ## Arguments
///
/// * `liquidity_delta` - Liquidity delta
/// * `liquidity` - Liquidity
/// * `volume_usd` - Volume in USD
/// * `fee` - Fee tier
pub fn estimate_fees_usd(
    liquidity_delta: U256,
    liquidity: U256,
    volume_usd: f64,
    fee: u32,
) -> BigDecimal {
    let fee_percentage = match fee {
        100 => BigDecimal::from_f64(0.01 / 100.0).unwrap(), // 0.0001
        500 => BigDecimal::from_f64(0.05 / 100.0).unwrap(), // 0.0005
        3000 => BigDecimal::from_f64(0.3 / 100.0).unwrap(), // 0.003
        10000 => BigDecimal::from_f64(1.0 / 100.0).unwrap(), // 0.01
        _ => panic!("Invalid fee tier"),
    };

    let liquidity_decimal = BigDecimal::from_str(&liquidity.to_string()).unwrap();
    let liquidity_delta_decimal = BigDecimal::from_str(&liquidity_delta.to_string()).unwrap();

    let liquidity_percentage =
        liquidity_delta_decimal.clone() / (liquidity_decimal + liquidity_delta_decimal);

    let volume_usd_decimal = BigDecimal::from_f64(volume_usd).unwrap();

    let earned_fees = fee_percentage * (volume_usd_decimal * liquidity_percentage);

    earned_fees
}

/// Estimate the earned fees in token values
pub fn estimate_fees_in_tokens(
    liquidity_delta: U256,
    liquidity: U256,
    buy_volume: f64,
    sell_volume: f64,
    fee: u32,
) -> (f64, f64) {
    let fee_percentage: f64 = match fee {
        100 => 0.01 / 100.0,
        500 => 0.05 / 100.0,
        3000 => 0.3 / 100.0,
        10000 => 1.0 / 100.0,
        _ => panic!("Invalid fee tier"),
    };

    let liquidity_f64 = liquidity.to_string().parse::<f64>().unwrap();
    let liquidity_delta_f64 = liquidity_delta.to_string().parse::<f64>().unwrap();

    let liquidity_percentage = liquidity_delta_f64 / (liquidity_f64 + liquidity_delta_f64);

    let earned_usdc_fees = fee_percentage * (buy_volume) * liquidity_percentage;
    let earned_usdt_fees = fee_percentage * (sell_volume) * liquidity_percentage;

    (earned_usdc_fees, earned_usdt_fees)
}

/// Get the amount of tokens to deposit
///
/// # Arguments
///
/// * `p` - Most active price assumption
/// * `pl` - Lower price range
/// * `pu` - Upper price range
/// * `token_a_price` - Token A price in usd
/// * `token_b_price` - Token B price in usd
/// * `deposit_amount` - Amount of deposit in usd
pub fn get_tokens_deposit_amount(
    p: f64,
    pl: f64,
    pu: f64,
    token_a_price: f64,
    token_b_price: f64,
    deposit_amount: f64,
) -> DepositAmounts {
    let delta_l = deposit_amount
        / ((p.sqrt() - pl.sqrt()) * token_b_price
            + (1.0 / p.sqrt() - 1.0 / pu.sqrt()) * token_a_price);

    let mut delta_y = delta_l * (p.sqrt() - pl.sqrt());

    if delta_y * token_b_price < 0.0 {
        delta_y = 0.0;
    }

    if delta_y * token_b_price > deposit_amount {
        delta_y = deposit_amount / token_b_price;
    }

    let mut delta_x = delta_l * (1.0 / p.sqrt() - 1.0 / pu.sqrt());

    if delta_x * token_a_price < 0.0 {
        delta_x = 0.0;
    }

    if delta_x * token_a_price > deposit_amount {
        delta_x = deposit_amount / token_a_price;
    }

    DepositAmounts {
        amount0: delta_x,
        amount1: delta_y,
        liquidity_delta: delta_l,
    }
}

/// Get the liquidity delta
///
/// # Arguments
///
/// * `p` - Most active price assumption
/// * `pl` - Lower price range
/// * `pu` - Upper price range
/// * `amount0` - Amount of token0
/// * `amount1` - Amount of token1
pub fn get_liquidity_delta(p: f64, pl: f64, pu: f64, amount0: U256, amount1: U256) -> U256 {
    let sqrt_ratio_x96 = get_sqrt_price_x96(p);
    let sqrt_ratio_lower_x96 = get_sqrt_price_x96(pl);
    let sqrt_ratio_upper_x96 = get_sqrt_price_x96(pu);

    if sqrt_ratio_x96 < sqrt_ratio_lower_x96 {
        return get_liquidity_for_amount0(sqrt_ratio_lower_x96, sqrt_ratio_upper_x96, amount0);
    } else if sqrt_ratio_x96 < sqrt_ratio_upper_x96 {
        let liquidity0 = get_liquidity_for_amount0(sqrt_ratio_x96, sqrt_ratio_upper_x96, amount0);
        let liquidity1 = get_liquidity_for_amount1(sqrt_ratio_lower_x96, sqrt_ratio_x96, amount1);
        return liquidity0.min(liquidity1);
    } else {
        return get_liquidity_for_amount1(sqrt_ratio_lower_x96, sqrt_ratio_upper_x96, amount1);
    }
}

/// Function to calculate liquidity for a given amount of token0
fn get_liquidity_for_amount0(
    sqrt_ratio_lower_x96: U256,
    sqrt_ratio_upper_x96: U256,
    amount0: U256,
) -> U256 {
    let intermediate = sqrt_ratio_upper_x96
        .checked_mul(sqrt_ratio_lower_x96)
        .unwrap()
        .checked_div(Q96)
        .unwrap();

    let numerator = amount0.checked_mul(intermediate).unwrap();

    let denominator = sqrt_ratio_upper_x96
        .checked_sub(sqrt_ratio_lower_x96)
        .unwrap();

    numerator.checked_div(denominator).unwrap()
}

fn get_liquidity_for_amount1(sqrt_ratio_ax96: U256, sqrt_ratio_bx96: U256, amount1: U256) -> U256 {
    let numerator = amount1
        .checked_mul(Q96)
        .unwrap();
    let denominator = sqrt_ratio_bx96.checked_sub(sqrt_ratio_ax96).unwrap();

    numerator.checked_div(denominator).unwrap()
}

pub fn get_liquidity_from_tick(pool_ticks: Vec<PoolTick>, current_tick: i32) -> U256 {
    let mut liquidity = 0_i128;

    for i in 0..pool_ticks.len() {
        liquidity += pool_ticks[i].liquidity_net;

        let lower_tick = pool_ticks[i].tick;
        let upper_tick = pool_ticks.get(i + 1).map(|t| t.tick).unwrap_or(lower_tick);

        // If the current tick lies between the lower and upper tick, we stop accumulating
        if lower_tick <= current_tick && current_tick <= upper_tick {
            break;
        }
    }

    U256::from(liquidity.abs() as u128)
}

/// Get the sqrt price x96
pub fn get_sqrt_price_x96(price: f64) -> U256 {
    let sqrt_price = price.sqrt();
    let scaled_price = sqrt_price * (2_u128.pow(96) as f64);

    U256::from(scaled_price as u128)
}

/// Calculate the tick from a given price
pub fn get_tick_from_price(price: f64) -> i32 {
    let sqrt_price = price.sqrt();

    let tick = (sqrt_price.ln() / (1.0001_f64).sqrt().ln()).round() as i32;

    tick
}