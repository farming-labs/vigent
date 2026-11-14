//! The template picker (`:templates`, T on the start screen): the shared
//! picker, as src/app.ts openTemplatePicker opens it. The templates
//! themselves, `:new -t` and the built-in four are the Sessions area's
//! (agents.rs: `all_templates`, `start_from_template`).

use crate::actions::windows::open_picker;
use crate::agents::{NewAgent, all_templates};
use crate::app::{App, Mode};
use crate::ui::picker::{Choice, Picker, Place};

pub fn open(app: &mut App) {
    let items = all_templates(&app.config.templates)
        .into_iter()
        .map(|(t, is_builtin)| {
            let how: Vec<String> = [t.engine.as_deref().map(crate::engines::engine_name), t.model.clone(), t.effort.clone()].into_iter().flatten().filter(|s| !s.is_empty()).collect();
            let what = t.description.clone().unwrap_or_else(|| t.prompt.lines().next().unwrap_or_default().to_string());
            let how = if how.is_empty() { String::new() } else { format!(" ({})", how.join(" · ")) };
            let yours = if is_builtin { "" } else { " · yours" };
            Choice::new(&t.name, None, Some(&format!("{what}{how}{yours}")))
        })
        .collect();
    let mut p = Picker::new(
        "Start an agent from a template",
        Some(items),
        None,
        false,
        Box::new(|app, v| {
            if let Some(name) = v {
                choose(app, &name);
            }
        }),
    );
    p.place = Place::Center { width: 96 };
    open_picker(app, p);
}

fn choose(app: &mut App, name: &str) {
    let list = all_templates(&app.config.templates);
    let found = list.iter().find(|(t, _)| t.name == name).cloned();
    if let Some((t, _)) = &found
        && !t.prompt.contains("{task}")
    {
        app.start_from_template(name, "", NewAgent::default());
        return;
    }
    app.mode = Mode::Command;
    app.cmd = format!("new -t {name} ");
    let review = found.is_some_and(|(t, b)| b && t.name == "review");
    app.say(if review { "what to focus on (optional), then Enter" } else { "what it's for, then Enter" }, "Comment");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::windows::picker_key;

    #[test]
    fn picking_a_template_that_needs_a_task_asks_for_it() {
        let mut a = crate::app::tests::app();
        open(&mut a);
        picker_key(&mut a, "j");
        picker_key(&mut a, "<CR>");
        assert_eq!((a.mode, a.cmd.as_str()), (Mode::Command, "new -t test "));
        assert!(a.windows.picker.is_none());
    }
}
