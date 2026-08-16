//! A number with a slider and the value beside it.

use zgui::prelude::*;
use zgui::{component, view};
use zgui_ui::prelude::*;
use zgui_ui_primitives::Binding;

/// A number with a slider and the value beside it.
///
/// The library has a text field and a slider, and no number input. A setting like a tab width
/// wants both: the slider to change it without thinking, and the number to know what it is.
#[component]
pub(crate) fn Number(
    /// The value.
    #[prop(into)]
    value: Binding<f64>,
    /// The smallest it may be.
    min: f64,
    /// The largest.
    max: f64,
    /// How far one keystroke moves it.
    step: f64,
    /// What the number is measured in, shown after it. Empty for a bare count.
    #[prop(into)]
    unit: String,
) -> impl IntoView {
    let showing = Signal::derive_local(move || {
        let held = value.get().unwrap_or_default();
        // Whole numbers as whole numbers: a tab width of `4` should not read as `4.0`.
        if (held - held.round()).abs() < f64::EPSILON {
            format!("{}", held.round() as i64)
        } else {
            format!("{held:.1}")
        }
    });

    view! {
        row(class = "config__number") {
            Slider(
                class = "config__slider",
                value = value,
                min = min,
                max = max,
                step = step,
                {..use_settings_item_attrs()}
            )
            label(class = "config__value nowrap") {
                {move || match unit.as_str() {
                    "" => showing.get(),
                    unit => format!("{} {unit}", showing.get()),
                }}
            }
        }
    }
}
