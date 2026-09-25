//! Adapted from Koharu's layout helper, revision c697b31eb1de016d9272743a2973f0e6a67eae6c.
//! Copyright (c) 2025-2026 Mayo Takanashi and Koharu contributors. MIT.
//! See third-party/koharu-LICENSE-MIT.
use crate::Result;
pub(crate) fn largest_fitting_font_size<T>(
    minimum: f32,
    maximum: f32,
    mut layout_at: impl FnMut(f32) -> Result<T>,
    fits: impl Fn(&T) -> bool,
) -> Result<Option<T>> {
    const PROBES: usize = 8;
    if maximum - minimum <= f32::EPSILON {
        let candidate = layout_at(maximum)?;
        return Ok(fits(&candidate).then_some(candidate));
    }
    let step = (maximum - minimum) / PROBES as f32;
    let mut larger_non_fit = None;
    for probe in 0..=PROBES {
        let size = if probe == PROBES {
            minimum
        } else {
            maximum - step * probe as f32
        };
        let candidate = layout_at(size)?;
        if fits(&candidate) {
            let Some(mut high) = larger_non_fit else {
                return Ok(Some(candidate));
            };
            let mut low = size;
            let mut best = candidate;
            let mut iterations = 0u32;
            while high - low > 0.01 && iterations < 10 {
                iterations += 1;
                let midpoint = (low + high) * 0.5;
                let candidate = layout_at(midpoint)?;
                if fits(&candidate) {
                    best = candidate;
                    low = midpoint;
                } else {
                    high = midpoint;
                }
            }
            return Ok(Some(best));
        }

        larger_non_fit = Some(size);
    }
    Ok(None)
}
