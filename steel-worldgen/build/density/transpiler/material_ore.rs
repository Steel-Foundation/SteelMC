//! bounds the materialore prefill to y levels where a rule may accept a block

use std::collections::BTreeMap;

use crate::density::DensityFunction;

/// height of the required dense prefix starting at `min_y`
pub(crate) fn value_height(
    densities: &[DensityFunction],
    registry: &BTreeMap<String, DensityFunction>,
    min_y: i32,
    height: usize,
) -> usize {
    let mut visiting = Vec::new();
    for relative_y in (0..height).rev() {
        let Some(y) = i32::try_from(relative_y)
            .ok()
            .and_then(|offset| min_y.checked_add(offset))
        else {
            return height;
        };

        if densities.iter().any(|density| {
            !constant_at_y(density, registry, y, &mut visiting).is_some_and(|value| value <= 0.0)
        }) {
            return relative_y + 1;
        }
    }

    0
}

fn constant_at_y<'a>(
    density: &'a DensityFunction,
    registry: &'a BTreeMap<String, DensityFunction>,
    y: i32,
    visiting: &mut Vec<&'a str>,
) -> Option<f32> {
    match density {
        DensityFunction::Constant(value) => Some(value.value as f32),
        DensityFunction::Reference(reference) => {
            if visiting.contains(&reference.id.as_str()) {
                return None;
            }

            let target = registry.get(&reference.id)?;
            visiting.push(&reference.id);

            let value = constant_at_y(target, registry, y, visiting);
            visiting.pop();

            value
        }
        DensityFunction::YClampedGradient(gradient) => {
            let span = gradient.to_y.checked_sub(gradient.from_y)?;
            if span <= 0 {
                return None;
            }

            let coordinate = y
                .clamp(gradient.from_y, gradient.to_y)
                .checked_sub(gradient.from_y)?;

            let from = gradient.from_value as f32;
            Some(from + coordinate as f32 * ((gradient.to_value as f32 - from) / span as f32))
        }
        DensityFunction::RangeChoice(choice) => {
            let input = constant_at_y(&choice.input, registry, y, visiting)?;
            let branch =
                if input >= choice.min_inclusive as f32 && input < choice.max_exclusive as f32 {
                    &choice.when_in_range
                } else {
                    &choice.when_out_of_range
                };

            constant_at_y(branch, registry, y, visiting)
        }
        _ => None,
    }
}
