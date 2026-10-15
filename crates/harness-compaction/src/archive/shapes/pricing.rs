//! Square and actual-size visual token prices per billing lineage.
use super::{ArchiveShape, Pricing};

pub(super) const fn square_price(edge: u32, pricing: Pricing) -> u32 {
    match pricing {
        Pricing::Unknown => 5024,
        Pricing::Google => 1120,
        Pricing::AnthropicHigh | Pricing::AnthropicOld => {
            let (max_edge, max_tokens) = if matches!(pricing, Pricing::AnthropicOld) {
                (1568, 1568)
            } else {
                (2576, 4784)
            };
            let mut e = edge;
            while e.div_ceil(28) * 28 > max_edge || e.div_ceil(28) * e.div_ceil(28) > max_tokens {
                e -= 1;
            }
            e.div_ceil(28) * e.div_ceil(28)
        }
        Pricing::Grok => {
            let n = edge.div_ceil(32) * edge.div_ceil(32);
            let n = if n > 2401 { 2401 } else { n };
            (n * 1017).div_ceil(1000)
        }
        _ => {
            let limit = if matches!(pricing, Pricing::OpenAiLimited) {
                2048
            } else if matches!(pricing, Pricing::OpenAi) {
                6000
            } else {
                65535
            };
            let e = if edge > limit { limit } else { edge };
            (e.div_ceil(32) * e.div_ceil(32) * 6).div_ceil(5)
        }
    }
}
/// Price actual frame dimensions using the resolved model's billing lineage.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Image dimensions and patch caps bound all numeric conversions"
)]
pub fn frame_tokens(shape: &ArchiveShape, width: u32, height: u32, _provider_family: &str) -> u32 {
    use Pricing as P;
    if width == 0 || height == 0 {
        return 0;
    }
    match shape.pricing {
        P::Unknown => 5024,
        P::Google => 1120,
        P::AnthropicHigh | P::AnthropicOld => {
            let (edge, cap) = if shape.pricing == P::AnthropicOld {
                (1568, 1568)
            } else {
                (2576, 4784)
            };
            let patches = |w: u32, h: u32| w.div_ceil(28) * h.div_ceil(28);
            let fits = |w: u32, h: u32| {
                w.div_ceil(28) * 28 <= edge && h.div_ceil(28) * 28 <= edge && patches(w, h) <= cap
            };
            if fits(width, height) {
                return patches(width, height);
            }
            let (long, short) = (width.max(height), width.min(height));
            let mut lo = 1;
            let mut hi = long;
            while lo < hi {
                let mid = lo + (hi - lo).div_ceil(2);
                let small = (f64::from(mid) * f64::from(short) / f64::from(long))
                    .round_ties_even()
                    .max(1.0) as u32;
                if fits(mid, small) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            patches(
                lo,
                (f64::from(lo) * f64::from(short) / f64::from(long))
                    .round_ties_even()
                    .max(1.0) as u32,
            )
        }
        P::OpenAi | P::OpenAiLimited | P::OpenAiUnlimited | P::Grok => {
            let (edge, cap, mult) = match shape.pricing {
                P::OpenAi => (6000, 10000, 1.2),
                P::OpenAiLimited => (2048, 6144, 1.2),
                P::Grok => (65535, 2401, 1.017),
                _ => (65535, u32::MAX, 1.2),
            };
            let scale = (f64::from(edge) / f64::from(width.max(height))).min(1.0);
            let mut w = (f64::from(width) * scale).round().max(1.0) as u32;
            let mut h = (f64::from(height) * scale).round().max(1.0) as u32;
            if w.div_ceil(32) * h.div_ceil(32) > cap {
                let mut shrink = (1024.0 * f64::from(cap) / (f64::from(w) * f64::from(h))).sqrt();
                let x = f64::from(w) * shrink / 32.0;
                let y = f64::from(h) * shrink / 32.0;
                shrink *= (x.floor() / x).min(y.floor() / y);
                w = (f64::from(w) * shrink).floor() as u32;
                h = (f64::from(h) * shrink).floor() as u32;
                if w == 0 || h == 0 {
                    return (f64::from(cap) * mult).ceil() as u32;
                }
            }
            (f64::from((w.div_ceil(32) * h.div_ceil(32)).min(cap)) * mult).ceil() as u32
        }
    }
}
