//! Which engine, model and effort agents run with (src/app.ts startAgent,
//! modelTarget, chooseModel, chooseOtherEngine, allModelChoices, chooseEffort,
//! openEnginePicker, engineNote, chooseEngine, openModelPicker,
//! openEffortPicker, noEngineReady; and cli.ts newSession's model rules).

use super::choices::{self, Choice, model_label};
use super::handoff::{self, ModelPick};
use crate::actions::windows::{fill_picker, open_picker};
use crate::ui::picker::Picker;
use super::{Launch, all_ids, default_engine, engine_name, intern, is_engine_id};
use crate::app::{App, Msg};
use crate::session::Session;

/// What a new agent asks for; None: what :engine, :model and :effort (or config) set.
#[derive(Debug, Clone, Default)]
pub struct StartOpts {
    pub engine: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// "no engine is signed in: run claude auth login or codex login, or for pi export ANTHROPIC_API_KEY=… · :checkhealth"
pub fn no_engine_ready(app: &App) -> String {
    let e = &app.engines.infos;
    let logins: Vec<&str> = e.iter().filter(|x| x.id != "pi" && x.installed).filter_map(|x| x.connect.as_deref()).collect();
    let pi = e.iter().find(|x| x.id == "pi").and_then(|x| x.connect.as_deref());
    let mut ways = vec![];
    if !logins.is_empty() {
        ways.push(format!("run {}", logins.join(" or ")));
    }
    if let Some(p) = pi {
        ways.push(format!("for pi {p}"));
    }
    let ways = ways.join(", or ");
    format!("no engine is signed in{} · :checkhealth", if ways.is_empty() { String::new() } else { format!(": {ways}") })
}

/// The engine a new agent runs on, or why it can't start (TS startAgent's checks).
pub fn engine_for_new(app: &App, opts: &StartOpts) -> Result<&'static str, String> {
    let engine = match &opts.engine {
        Some(e) => intern(e).ok_or_else(|| format!("unknown engine '{e}': pi, claude, codex or opencode"))?,
        None => app.new_agent_engine(),
    };
    if let Some(info) = app.engines.get(engine) {
        if !info.installed {
            return Err(format!("{} isn't installed: {} · :checkhealth", info.name, info.install.clone().unwrap_or_default()));
        }
        // auto lands on an engine that isn't ready only when none is; an agent started there could never answer
        if opts.engine.is_none() && app.engines.preferred == "auto" && !info.connected && !info.checking {
            return Err(no_engine_ready(app));
        }
    }
    Ok(engine)
}

/// Why a new agent can't start (on `engine`, or the default one), if it can't.
pub fn cant_start(app: &App, engine: Option<&str>) -> Option<String> {
    engine_for_new(app, &StartOpts { engine: engine.map(str::to_string), ..Default::default() }).err()
}

/// A new agent's model and effort: what was asked, else what :model / :effort / config set for its engine.
pub fn configure_new(app: &App, s: &mut Session, opts: &StartOpts) {
    let model = opts.model.clone().or_else(|| app.engines.new_model.get(&s.engine).cloned());
    s.requested_model = model.filter(|m| m != "default");
    s.effort = opts.effort.clone().or_else(|| app.engines.new_effort.get(&s.engine).cloned());
}

/// How an agent's CLI runs now: [engines.<id>] args and allow (read at each prompt: :permissions changes them), and detection's dropEnv.
pub fn launch(app: &App, engine: &str) -> Launch {
    let o = app.config.engine_options(engine);
    Launch {
        bin: super::bin(engine),
        extra_args: o.and_then(|o| o.args.clone()),
        allow: o.and_then(|o| o.allow.clone()).unwrap_or_default(),
        ask: o.and_then(|o| o.ask).unwrap_or(false),
        drop_env: app.engines.get(engine).map(|e| e.drop_env.clone()).unwrap_or_default(),
        blocked: app.untrusted(),
    }
}

/// Who `:model` / `:effort` change: the focused agent, or new agents on the default engine.
#[derive(Clone)]
pub struct Target {
    pub session: Option<usize>,
    pub engine: String,
    pub label: String,
}

pub fn target(app: &App) -> Target {
    match app.focused() {
        Some(s) => Target { session: Some(s.slot), engine: s.engine.clone(), label: format!("'{}' ({})", s.name, engine_name(&s.engine)) },
        None => {
            let e = app.new_agent_engine();
            Target { session: None, engine: e.into(), label: format!("new {} agents", engine_name(e)) }
        }
    }
}

/// Save an agent's model and effort with its record, so they survive a restart (cli.ts app.persist; agents.rs).
pub fn persist(app: &App, i: usize) {
    app.persist(i);
}

/// `:model <id>` (TS chooseModel). With an agent open, an exact id another
/// engine lists moves the agent to that engine (`switch_engine`); anything
/// else is a model for its own engine. With none, the model for new agents.
pub fn choose_model(app: &mut App, model: &str, t: &Target) {
    let Some(i) = t.session else { return set_model(app, model, t) };
    if model == "default" {
        return set_model(app, model, t);
    }
    if app.engines.models.is_none() {
        // which engine lists it comes from the engines' own lists, which take a moment the first time
        app.say(format!("looking up {model}…"), "Comment");
    }
    let (model, t) = (model.to_string(), t.clone());
    with_models(
        app,
        Box::new(move |app| {
            if app.sessions.get(i).is_none_or(|s| s.closed) {
                return;
            }
            let own = app.sessions[i].engine.clone();
            let index = app.engines.models.clone().unwrap_or_default();
            let ready = |e: &str| app.engines.get(e).is_none_or(|x| x.connected || x.checking);
            match handoff::resolve_model(&model, &own, &index, &ready) {
                Some(to) if to.engine != own => switch_engine(app, i, to),
                _ => set_model(app, &model, &t),
            }
        }),
    );
}

/// A row of the :model picker: one of the agent's own engine's models, or another engine's (`engine:model`).
fn pick_model(app: &mut App, value: &str, t: &Target, labels: &[(String, String)]) {
    if let Some((e, m)) = value.split_once(':')
        && !e.is_empty()
        && is_engine_id(e)
    {
        if e != t.engine {
            let Some(i) = t.session else { return choose_other_engine(app, e, m) };
            let label = labels.iter().find(|(v, _)| v == value).map(|(_, l)| l.clone());
            return switch_engine(app, i, ModelPick { engine: e.into(), model: m.into(), label });
        }
        return set_model(app, m, t);
    }
    set_model(app, value, t);
}

/// A model for the agent's own engine (or new agents' engine), from the next prompt.
fn set_model(app: &mut App, model: &str, t: &Target) {
    let value = (model != "default").then(|| model.to_string());
    let name = value.as_deref().map(model_label).unwrap_or_else(|| "default".into());
    let Some(i) = t.session else {
        match &value {
            Some(v) => app.engines.new_model.insert(t.engine.clone(), v.clone()),
            None => app.engines.new_model.remove(&t.engine),
        };
        return app.say(format!("model for new agents: {name}"), "ModeMsg");
    };
    // the last pick wins: a move waiting for this turn to end is off
    app.sessions[i].pending_switch = None;
    let done = move |app: &mut App, r: Result<(), String>| match r {
        Ok(()) => {
            persist(app, i);
            let msg = format!("model for '{}': {name} (from the next prompt)", app.sessions[i].name);
            app.say(msg, "ModeMsg");
        }
        Err(e) => app.say(e, "ErrorMsg"),
    };
    if app.sessions[i].engine == "pi" {
        super::pi::set_model(app, i, value, Box::new(done));
    } else {
        app.sessions[i].requested_model = value;
        app.sessions[i].model_name = None;
        done(app, Ok(()));
    }
}

/// A model from another engine's list, with no agent open: new agents run on that engine with it.
fn choose_other_engine(app: &mut App, engine: &str, model: &str) {
    let Some(id) = intern(engine) else { return };
    app.engines.preferred = id;
    if model == "default" {
        app.engines.new_model.remove(id);
    } else {
        app.engines.new_model.insert(id.into(), model.into());
    }
    let name = if model == "default" { "the default model".to_string() } else { model_label(model) };
    app.say(format!("new agents use {name} · :new <task> starts one"), "ModeMsg");
}

/// Every engine's models with their names, loaded once (on a thread, the first time); `then` runs once they're in.
pub fn with_models(app: &mut App, then: Box<dyn FnOnce(&mut App)>) {
    if app.engines.models.is_some() {
        return then(app);
    }
    if let Some(waiting) = app.engines.models_waiting.as_mut() {
        return waiting.push(then);
    }
    app.engines.models_waiting = Some(vec![then]);
    let (infos, env, tx) = (app.engines.infos.clone(), app.engines.env.clone(), app.tx.clone());
    std::thread::spawn(move || {
        let index: handoff::ModelIndex = all_ids()
            .into_iter()
            .map(|e| {
                // an engine that isn't installed can't list its models; Claude Code's and Gemini CLI's lists are fixed, so they still say whose a model is
                let missing = infos.iter().find(|i| i.id == e).is_some_and(|i| !i.installed) && e != "claude" && e != "gemini";
                let list = if missing { vec![] } else { choices::model_choices(e, &env).into_iter().filter(|c| c.value != "default").collect() };
                (e.to_string(), list)
            })
            .collect();
        let _ = tx.send(Msg::Run(Box::new(move |app| {
            app.engines.models = Some(index);
            for then in app.engines.models_waiting.take().unwrap_or_default() {
                then(app);
            }
        })));
    });
}

/// Each engine's model ids (`:race --models`).
pub fn model_ids(index: &handoff::ModelIndex) -> Vec<(String, Vec<String>)> {
    index.iter().map(|(e, l)| (e.clone(), l.iter().map(|c| c.value.clone()).collect())).collect()
}

/// In the middle of something: a turn, its CLI wrapping up, a question for you, its worktree being made.
fn busy(s: &Session) -> bool {
    s.busy() || s.finishing || s.state == crate::session::State::Waiting
}

/// Move agent `i` to a model on another engine, in place: same name, worktree,
/// branch, place in the sidebar and transcript (TS switchEngine). Its old
/// engine's session goes, and the conversation goes along as text with the
/// next prompt. While it's busy the move waits for the turn to end.
pub fn switch_engine(app: &mut App, i: usize, to: ModelPick) {
    if let Some(why) = app.untrusted() {
        return app.say(why, "ErrorMsg");
    }
    if app.race_of(Some(i)).is_some() {
        return app.say("a racer keeps its engine; :new starts an agent on another one", "ErrorMsg");
    }
    if let Some(info) = app.engines.get(&to.engine)
        && !info.connected
        && !info.checking
    {
        // what :checkhealth says about it
        let how = match (&info.install, &info.connect) {
            (Some(i), _) if !info.installed => format!(" · install: {i}"),
            (_, Some(c)) => format!(" · connect: {c}"),
            _ => String::new(),
        };
        let msg = format!("can't switch to {}: {}{how}", info.name, info.detail);
        return app.say(msg, "ErrorMsg");
    }
    if busy(&app.sessions[i]) {
        let msg = format!("switches to {} after this turn", handoff::model_phrase(&engine_name(&to.engine), Some(&to.model), to.label.as_deref()));
        app.sessions[i].pending_switch = Some(to);
        return app.say(msg, "ModeMsg");
    }
    move_session(app, i, to);
}

/// A move that waited for agent `i`'s turn: it happens once the agent is free.
pub fn settle_switch(app: &mut App, i: usize) {
    let s = &app.sessions[i];
    if s.pending_switch.is_none() || s.closed || busy(s) {
        return;
    }
    let to = app.sessions[i].pending_switch.take().expect("pending");
    move_session(app, i, to);
}

/// The move itself (TS moveSession): the agent's engine, model and effort change, its old engine's session stops.
fn move_session(app: &mut App, i: usize, to: ModelPick) {
    let s = &app.sessions[i];
    let from_name = engine_name(&s.engine);
    let from_model = s.requested_model.clone().or_else(|| s.model.clone()).filter(|m| !m.is_empty() && m != "default" && m != "pi" && *m != from_name);
    let from_label = s.model_name.clone().or_else(|| {
        let m = from_model.as_deref()?;
        app.engines.models.as_ref()?.iter().find(|(e, _)| *e == s.engine)?.1.iter().find(|c| c.value == m)?.label.clone()
    });
    let effort = s.effort.clone().filter(|e| super::efforts(&to.engine).contains(&e.as_str()));
    // a handoff that hasn't gone out yet still holds the whole conversation
    let carried = s.handoff.clone().or_else(|| Some(handoff::handoff_text(&s.items, &from_name, from_model.as_deref())).filter(|h| !h.is_empty()));
    let now = handoff::model_phrase(&engine_name(&to.engine), Some(&to.model), to.label.as_deref());
    let s = &mut app.sessions[i];
    // its old engine's session stops: the CLI's conversation id goes, pi's process is stopped
    s.kill_child();
    s.engine = to.engine.clone();
    s.requested_model = Some(to.model.clone());
    s.model = None;
    s.model_name = to.label.clone();
    s.effort = effort;
    s.resume_id = None;
    s.last_message = None;
    s.forking = false;
    s.fork_at = None;
    s.after_exit = None;
    s.ask = None;
    s.thinking = false;
    s.finishing = false;
    s.launch = Default::default();
    s.pending_switch = None;
    s.handoff = carried;
    s.pi_fresh = to.engine == "pi";
    if s.state == crate::session::State::Error {
        s.state = crate::session::State::Idle;
    }
    s.items.push(crate::session::Item::Info(format!("switched from {} to {now}; the next prompt carries the conversation over", handoff::model_phrase(&from_name, from_model.as_deref(), from_label.as_deref()))));
    let name = s.name.clone();
    persist(app, i);
    app.say(format!("'{name}' now runs {now}"), "ModeMsg");
    if to.engine == "pi" {
        // pi starts now, as a new pi agent does
        super::pi::warm(app, i);
    }
}

/// What :model lists: the target engine's models first, then those of every other
/// engine that's ready, in one plain list (TS allModelChoices). Blocking.
pub fn all_model_choices(engine: &str, ready: &[String], env: &super::Env) -> Vec<Choice> {
    let mut order = vec!["claude", "codex", "opencode", "pi"];
    order.extend(all_ids().into_iter().filter(|e| !["claude", "codex", "opencode", "pi"].contains(e)));
    let others: Vec<&str> = order.into_iter().filter(|e| *e != engine && ready.iter().any(|r| r == e)).collect();
    let mut out = vec![];
    for (k, e) in std::iter::once(engine).chain(others.iter().copied()).enumerate() {
        for c in choices::model_choices(e, env) {
            if k > 0 && c.value == "default" {
                continue;
            }
            let value = if k == 0 { c.value.clone() } else { format!("{e}:{}", c.value) };
            let note = (c.value != "default").then(|| c.value.clone());
            out.push(Choice { value, label: c.label.clone(), note, group: None });
        }
    }
    out
}

/// `:model`: pick from what the engines offer, or type any name (TS openModelPicker).
pub fn open_model_picker(app: &mut App) {
    let t = target(app);
    let current = match t.session {
        Some(i) => {
            let s = &app.sessions[i];
            if s.engine == "pi" { s.requested_model.clone().or_else(|| s.model.clone()).unwrap_or_else(|| "default".into()) } else { s.requested_model.clone().unwrap_or_else(|| "default".into()) }
        }
        None => app.engines.new_model.get(&t.engine).cloned().unwrap_or_else(|| "default".into()),
    };
    let title = match t.session {
        Some(i) => format!("Model · '{}'", app.sessions[i].name),
        None => "Model · new agents".into(),
    };
    let (engine, ready, env) = (t.engine.clone(), app.engines.infos.iter().filter(|e| e.connected).map(|e| e.id.clone()).collect::<Vec<_>>(), app.engines.env.clone());
    // each row's name, for the message when it moves the agent
    let labels = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, String)>::new()));
    let keep = std::sync::Arc::clone(&labels);
    let mut p = Picker::new(&title, None, Some(current), true, Box::new(move |app, v| {
        if let Some(v) = v {
            let labels = keep.lock().map(|l| l.clone()).unwrap_or_default();
            pick_model(app, &v, &t, &labels);
        }
    }));
    p.rows = 12;
    load_into(app, p, move || {
        let list = all_model_choices(&engine, &ready, &env);
        if let Ok(mut l) = labels.lock() {
            *l = list.iter().map(|c| (c.value.clone(), c.label.clone().unwrap_or_else(|| c.value.clone()))).collect();
        }
        list
    });
}

/// Open `p` and fill it from `load`, run on a thread.
fn load_into(app: &mut App, p: Picker, load: impl FnOnce() -> Vec<Choice> + Send + 'static) {
    let (id, tx) = (p.id, app.tx.clone());
    open_picker(app, p);
    std::thread::spawn(move || {
        let list = load();
        let _ = tx.send(Msg::Run(Box::new(move |app| fill_picker(app, id, list))));
    });
}

/// `:effort <level>` (TS chooseEffort).
pub fn choose_effort(app: &mut App, level: &str, t: &Target) {
    let mut allowed = vec!["default"];
    allowed.extend(super::efforts(&t.engine));
    // Codex's levels differ per model and version; Codex itself says if one doesn't fit
    let codex_word = t.engine == "codex" && !level.is_empty() && level.bytes().all(|b| b.is_ascii_lowercase());
    if !allowed.contains(&level) && !codex_word {
        return app.say(format!("{} takes: {}", engine_name(&t.engine), allowed.join(", ")), "ErrorMsg");
    }
    let value = (level != "default").then(|| level.to_string());
    let Some(i) = t.session else {
        match &value {
            Some(v) => app.engines.new_effort.insert(t.engine.clone(), v.clone()),
            None => app.engines.new_effort.remove(&t.engine),
        };
        return app.say(format!("effort for {}: {level}", t.label), "ModeMsg");
    };
    let msg = format!("effort for {}: {level} (from the next prompt)", t.label);
    let done = move |app: &mut App, r: Result<(), String>| match r {
        Ok(()) => {
            persist(app, i);
            app.say(msg, "ModeMsg");
        }
        Err(e) => app.say(e, "ErrorMsg"),
    };
    if app.sessions[i].engine == "pi" {
        super::pi::set_effort(app, i, value, Box::new(done));
    } else {
        app.sessions[i].effort = value;
        done(app, Ok(()));
    }
}

/// `:effort`: the reasoning effort / thinking levels the engine accepts (TS openEffortPicker).
pub fn open_effort_picker(app: &mut App) {
    let t = target(app);
    let (current, model) = match t.session {
        Some(i) => {
            let s = &app.sessions[i];
            (s.effort.clone(), s.requested_model.clone().or_else(|| if s.engine == "pi" { s.model.clone() } else { None }))
        }
        None => (app.engines.new_effort.get(&t.engine).cloned(), app.engines.new_model.get(&t.engine).cloned()),
    };
    let current = current.unwrap_or_else(|| "default".into());
    let (engine, env) = (t.engine.clone(), app.engines.env.clone());
    let title = format!("Effort · {}", t.label);
    let p = Picker::new(&title, None, Some(current), false, Box::new(move |app, v| {
        if let Some(v) = v {
            choose_effort(app, &v, &t);
        }
    }));
    load_into(app, p, move || choices::effort_choices_for(&engine, model.as_deref(), &env));
}

/// "Claude Code · ready", "Codex · not signed in: codex login", "pick for me (now Claude Code)" (TS engineNote).
pub fn engine_note(app: &App, id: &str) -> String {
    if id == "auto" {
        return format!("pick for me (now {})", engine_name(default_engine(&app.engines.infos)));
    }
    let Some(e) = app.engines.get(id) else { return engine_name(id) };
    let status = if e.connected {
        "ready".to_string()
    } else if !e.installed {
        "not installed".into()
    } else if let Some(c) = &e.connect {
        format!("not signed in: {c}")
    } else {
        "not signed in".into()
    };
    format!("{} · {status}", engine_name(id))
}

/// `:engine`: which engine new agents run on, with what's ready on this machine.
pub fn open_engine_picker(app: &mut App) {
    let mut items = vec![Choice::new("auto", None, Some(&engine_note(app, "auto")))];
    items.extend(all_ids().into_iter().map(|id| Choice::new(id, None, Some(&engine_note(app, id)))));
    let current = app.engines.preferred.to_string();
    let p = Picker::new("Engine for new agents", Some(items), Some(current), false, Box::new(|app, v| {
        if let Some(v) = v {
            choose_engine(app, &v);
        }
    }));
    open_picker(app, p);
}

/// `:engine <name>`: new agents run on it. Agents already open keep theirs (their conversation lives there).
pub fn choose_engine(app: &mut App, value: &str) {
    if value != "auto" && !is_engine_id(value) {
        return app.say(format!("engines: auto, {}", all_ids().join(", ")), "ErrorMsg");
    }
    app.engines.preferred = if value == "auto" { "auto" } else { intern(value).unwrap_or("auto") };
    let now = app.new_agent_engine();
    let warn = match app.engines.get(now) {
        Some(e) if !e.connected => {
            let how = if e.installed { format!("not signed in{}", e.connect.as_ref().map(|c| format!(": run {c}")).unwrap_or_default()) } else { "not installed".into() };
            format!(" ({how})")
        }
        _ => String::new(),
    };
    let open = match app.focused() {
        Some(s) if s.engine != now => format!(" · '{}' stays on {}; :new <task> starts one on {}", s.name, engine_name(&s.engine), engine_name(now)),
        _ => String::new(),
    };
    let auto = if value == "auto" { " (auto)" } else { "" };
    let group = if warn.is_empty() { "ModeMsg" } else { "WarningMsg" };
    app.say(format!("new agents run on {}{auto}{warn}{open} · :settings, w keeps it", engine_name(now)), group);
}

/// Claude Code refused agent `i` some tools ([engines.claude] ask = true): allow them for
/// one retry, always ([engines.claude] allow, for this session), or leave them blocked (TS askToAllow).
pub fn ask_to_allow(app: &mut App, i: usize) {
    let Some(ask) = app.sessions[i].ask.clone() else { return };
    let list = ask.missing.join(", ");
    let items = vec![
        Choice::new("allow once", None, Some(&format!("{list}, for this retry"))),
        Choice::new("always allow", None, Some(&format!("{list}, saved to [engines.claude] allow"))),
        Choice::new("no", None, Some("leave it blocked")),
    ];
    let title = format!("'{}' was blocked · {}", app.sessions[i].name, ask.what);
    let mut p = Picker::new(&title, Some(items), None, false, Box::new(move |app, v| {
        let s = &mut app.sessions[i];
        // aborted, or a new prompt was sent meanwhile
        if s.ask.take().is_none() || s.state != crate::session::State::Waiting {
            return;
        }
        let choice = v.as_deref().filter(|v| *v != "no");
        let Some(choice) = choice else {
            s.items.push(crate::session::Item::Info("left blocked".into()));
            s.state = crate::session::State::Idle;
            return app.drain(i);
        };
        let always = choice == "always allow";
        if always {
            // kept for this session; writing it to config.toml waits for the Config area's
            // line-by-line editor (src/config-edit.ts setTomlValue)
            let mut o = app.config.engine_options("claude").cloned().unwrap_or_default();
            let mut allow = o.allow.unwrap_or_default();
            for r in &ask.missing {
                if !allow.contains(r) {
                    allow.push(r.clone());
                }
            }
            o.allow = Some(allow);
            match app.config.engines.iter_mut().find(|(k, _)| k == "claude") {
                Some(slot) => slot.1 = o,
                None => app.config.engines.push(("claude".into(), o)),
            }
        }
        let how = if always { "always allowed" } else { "allowed once" };
        app.sessions[i].items.push(crate::session::Item::Info(format!("{how}: {} · retrying", ask.missing.join(", "))));
        // the retry runs with the rules allowed; it isn't a prompt of yours, so it stays out of the transcript
        let text = format!("You now have permission to use {}. Retry what was blocked, then carry on with the task.", ask.rules.join(", "));
        let mut l = launch(app, "claude");
        for r in &ask.missing {
            if !l.allow.contains(r) {
                l.allow.push(r.clone());
            }
        }
        let tx = app.tx.clone();
        let s = &mut app.sessions[i];
        s.send(text.clone(), &l, tx);
        if let Some(j) = s.items.iter().rposition(|x| *x == crate::session::Item::User(text.clone())) {
            s.items.remove(j);
        }
    }));
    p.rows = 3;
    open_picker(app, p);
}

/// Model and effort for new agents at startup: [engines.<id>] model/effort, `model` (pi's), and --model
/// (pi's, or the --engine CLI's) (cli.ts: newAgentModel, piModel, requested).
pub fn startup(app: &mut App, model_arg: Option<&str>, engine_arg: Option<&str>) {
    let cfg = app.config.clone();
    for (id, o) in &cfg.engines {
        if let Some(m) = &o.model {
            app.engines.new_model.insert(id.clone(), m.clone());
        }
        if let Some(e) = &o.effort {
            app.engines.new_effort.insert(id.clone(), e.clone());
        }
    }
    let pi = match (model_arg, engine_arg) {
        (Some(m), None | Some("pi")) => Some(m.to_string()),
        _ => cfg.engine_options("pi").and_then(|o| o.model.clone()).or(cfg.model.clone()),
    };
    if let Some(m) = pi {
        app.engines.new_model.insert("pi".into(), m);
    }
    if let (Some(m), Some(e)) = (model_arg, engine_arg)
        && e != "pi"
        && cfg.engine_options(e).and_then(|o| o.model.as_ref()).is_none()
    {
        app.engines.new_model.insert(e.into(), m.into());
    }
    // engine: --engine, else config's (auto lets default_engine() pick)
    let chosen = engine_arg.map(str::to_string).or_else(|| cfg.engine.clone().filter(|e| e != "auto"));
    app.engines.preferred = chosen.as_deref().and_then(intern).unwrap_or("auto");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::app;
    use crate::engines::EngineInfo;

    fn info(id: &str, installed: bool, connected: bool, connect: Option<&str>) -> EngineInfo {
        EngineInfo { id: id.into(), name: engine_name(id), installed, connected, connect: connect.map(str::to_string), ..Default::default() }
    }

    #[test]
    fn engine_picks_for_new_agents_and_leaves_open_ones_alone() {
        let mut a = app();
        a.engines.infos = vec![info("pi", true, true, None), info("claude", true, true, None), info("codex", true, false, Some("codex login")), info("opencode", false, false, None)];
        assert_eq!(engine_note(&a, "auto"), "pick for me (now Claude Code)");
        assert_eq!(engine_note(&a, "claude"), "Claude Code · ready");
        assert_eq!(engine_note(&a, "codex"), "Codex · not signed in: codex login");
        assert_eq!(engine_note(&a, "opencode"), "OpenCode · not installed");
        choose_engine(&mut a, "claude");
        assert_eq!(a.engines.preferred, "claude");
        assert!(a.message.0.starts_with("new agents run on Claude Code"));
        let mut s = Session::new(0, "old", "pi", "/nope".into());
        s.engine = "pi".into();
        a.sessions.push(s);
        a.focus = 0;
        choose_engine(&mut a, "codex");
        assert!(a.message.0.contains("new agents run on Codex (not signed in: run codex login)"));
        assert!(a.message.0.contains("'old' stays on pi; :new <task> starts one on Codex"));
        choose_engine(&mut a, "gpt");
        assert!(a.message.0.starts_with("engines: auto, pi, claude, codex, opencode"));
        assert_eq!(a.engines.preferred, "codex");
    }

    #[test]
    fn model_and_effort_with_no_agent_set_them_for_new_agents() {
        let mut a = app();
        a.engines.preferred = "codex";
        let t = target(&a);
        choose_model(&mut a, "gpt-5-codex", &t);
        choose_effort(&mut a, "xhigh", &t);
        assert_eq!(a.message.0, "effort for new Codex agents: xhigh");
        let mut s = Session::new(0, "x", "codex", "/nope".into());
        configure_new(&a, &mut s, &StartOpts::default());
        assert_eq!((s.requested_model.as_deref(), s.effort.as_deref()), (Some("gpt-5-codex"), Some("xhigh")));
    }

    #[test]
    fn model_and_effort_for_the_focused_agent() {
        let mut a = switch_app();
        a.sessions.push(Session::new(0, "racer", "claude", "/nope".into()));
        a.focus = 0;
        let t = target(&a);
        choose_model(&mut a, "claude-opus-5-5", &t);
        assert_eq!(a.message.0, "model for 'racer': Claude Opus 5.5 (from the next prompt)");
        choose_effort(&mut a, "high", &t);
        assert_eq!(super::super::model_text(&a.sessions[0]), "Claude Code · claude-opus-5-5 · high");
        choose_effort(&mut a, "off", &t);
        assert_eq!(a.message.0, "Claude Code takes: default, low, medium, high, xhigh, max");
        choose_effort(&mut a, "default", &t);
        assert_eq!(a.sessions[0].effort, None);
        // a name no engine lists: tried on its own engine, as before
        choose_model(&mut a, "my-own-model", &t);
        assert_eq!((a.sessions[0].engine.as_str(), a.sessions[0].requested_model.as_deref()), ("claude", Some("my-own-model")));
        assert_eq!(a.sessions[0].items, vec![], "no line in the transcript");
    }

    /// An app whose engines are all ready, with their models loaded.
    fn switch_app() -> App {
        let mut a = app();
        for e in &mut a.engines.infos {
            (e.installed, e.connected, e.checking) = (true, true, false);
        }
        let c = |v: &str, l: &str| Choice::new(v, Some(l), None);
        a.engines.models = Some(vec![
            ("pi".into(), vec![c("fake/test", "Test Model")]),
            ("claude".into(), choices::model_choices("claude", &crate::engines::detect::tests::env(&[("PATH", "")])).into_iter().filter(|c| c.value != "default").collect()),
            ("codex".into(), vec![c("gpt-5.5", "GPT-5.5"), c("gpt-5.5-codex", "GPT-5.5 Codex")]),
            ("gemini".into(), vec![c("pro", "Gemini Pro")]),
        ]);
        a
    }

    fn badge(a: &mut App, engine: &str, model: &str) {
        let mut s = Session::new(0, "badge", engine, "/nope".into());
        s.requested_model = Some(model.into());
        s.items = vec![
            Item::User("fix the badge".into()),
            Item::Tool { id: "t".into(), name: "Bash".into(), summary: "npm test".into() },
            Item::Text("It counts lines; it should sum quantities.".into()),
        ];
        a.sessions.push(s);
        a.focus = 0;
    }

    const HANDOFF: &str = "You're continuing a conversation that ran on Codex (gpt-5.5) in this same worktree. Here it is so far:\n\n<user>fix the badge</user>\nBash: npm test\n<assistant>It counts lines; it should sum quantities.</assistant>\n\nContinue from here. The files in this worktree are already as that conversation left them.\n\n";

    use crate::session::{Item, State};

    #[test]
    fn an_exact_id_another_engine_lists_moves_the_agent_in_place() {
        let mut a = switch_app();
        badge(&mut a, "codex", "gpt-5.5");
        a.sessions[0].effort = Some("xhigh".into());
        a.sessions[0].resume_id = Some("thread-1".into());
        let t = target(&a);
        choose_model(&mut a, "claude-opus-5-5", &t);
        let s = &a.sessions[0];
        assert_eq!((s.name.as_str(), s.engine.as_str(), s.requested_model.as_deref(), s.effort.as_deref()), ("badge", "claude", Some("claude-opus-5-5"), Some("xhigh")));
        assert_eq!(s.resume_id, None, "Codex's conversation stays behind");
        assert_eq!(s.handoff.as_deref(), Some(HANDOFF));
        assert_eq!(s.items.len(), 4, "the transcript stays");
        assert_eq!(s.items[3], Item::Info("switched from gpt-5.5 (GPT-5.5, Codex) to claude-opus-5-5 (Claude Opus 5.5, Claude Code); the next prompt carries the conversation over".into()));
        assert_eq!(a.message.0, "'badge' now runs claude-opus-5-5 (Claude Opus 5.5, Claude Code)");
        assert_eq!(super::super::model_text(s), "Claude Code · claude-opus-5-5 (Claude Opus 5.5) · xhigh", "the winbar");
        // an effort the new engine doesn't take goes back to its default
        a.sessions[0].effort = Some("max".into());
        let t = target(&a);
        choose_model(&mut a, "pro", &t);
        assert_eq!((a.sessions[0].engine.as_str(), a.sessions[0].effort.as_deref()), ("gemini", None));
        assert_eq!(a.sessions[0].handoff.as_deref(), Some(HANDOFF), "not prompted since: the same conversation still goes over");
        assert_eq!(a.message.0, "'badge' now runs pro (Gemini Pro, Gemini CLI)");
    }

    #[test]
    fn the_first_prompt_that_goes_out_carries_the_handoff_once() {
        let dir = std::env::temp_dir().join(format!("vigent-handoff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("args");
        let bin = dir.join("claude");
        std::fs::write(&bin, format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s\\0' \"$a\"; done > {}\n", log.display())).unwrap();
        std::process::Command::new("chmod").arg("755").arg(&bin).status().unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut s = Session::new(0, "badge", "claude", dir.clone());
        s.handoff = Some(HANDOFF.into());
        let launch = super::super::Launch { bin: bin.to_string_lossy().into_owned(), ..Default::default() };
        s.send("and the tests".into(), &launch, tx.clone());
        let _ = s.child.take().unwrap().wait();
        let args = std::fs::read_to_string(&log).unwrap();
        assert_eq!(args.split('\0').rev().nth(1), Some(format!("{HANDOFF}and the tests").as_str()));
        assert!(s.handoff_sent && s.handoff.is_none());
        assert_eq!(s.items.last(), Some(&Item::User("and the tests".into())), "the transcript shows what you typed");
        s.run("more".into(), &launch, tx);
        let _ = s.child.take().unwrap().wait();
        assert_eq!(std::fs::read_to_string(&log).unwrap().split('\0').rev().nth(1), Some("more"), "only the first");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn during_a_turn_the_switch_waits_and_the_last_pick_wins() {
        let mut a = switch_app();
        badge(&mut a, "gemini", "pro");
        a.sessions[0].state = State::Running;
        a.sessions[0].seen = State::Running;
        let t = target(&a);
        choose_model(&mut a, "gpt-5.5", &t);
        assert_eq!(a.message.0, "switches to gpt-5.5 (GPT-5.5, Codex) after this turn");
        choose_model(&mut a, "claude-haiku-5-5", &t);
        assert_eq!(a.message.0, "switches to claude-haiku-5-5 (Claude Haiku 5.5, Claude Code) after this turn");
        assert_eq!(a.sessions[0].engine, "gemini", "nothing changes mid-turn");
        a.sessions[0].queue.push_back("then the docs".into());
        a.sessions[0].state = State::Idle;
        settle_switch(&mut a, 0);
        let s = &a.sessions[0];
        assert_eq!((s.engine.as_str(), s.requested_model.as_deref()), ("claude", Some("claude-haiku-5-5")), "the last pick");
        assert!(s.handoff.as_deref().unwrap().starts_with("You're continuing a conversation that ran on Gemini CLI (pro) in this same worktree."));
        assert_eq!(s.queue.len(), 1, "the queued prompt goes to the new engine next, with the handoff");
        // a pick on its own engine while a move waits calls it off
        a.sessions[0].state = State::Running;
        let t = target(&a);
        choose_model(&mut a, "gpt-5.5", &t);
        choose_model(&mut a, "claude-sonnet-5-5", &t);
        a.sessions[0].state = State::Idle;
        settle_switch(&mut a, 0);
        assert_eq!((a.sessions[0].engine.as_str(), a.sessions[0].requested_model.as_deref()), ("claude", Some("claude-sonnet-5-5")));
    }

    #[test]
    fn refused_an_engine_that_isnt_ready_and_a_racer() {
        let mut a = switch_app();
        badge(&mut a, "claude", "claude-opus-5-5");
        let codex = a.engines.infos.iter_mut().find(|e| e.id == "codex").unwrap();
        (codex.connected, codex.detail, codex.connect) = (false, "not signed in".into(), Some("codex login".into()));
        let t = target(&a);
        choose_model(&mut a, "gpt-5.5", &t);
        assert_eq!(a.message.0, "can't switch to Codex: not signed in · connect: codex login", ":checkhealth's words");
        let codex = a.engines.infos.iter_mut().find(|e| e.id == "codex").unwrap();
        (codex.installed, codex.detail, codex.install) = (false, "not installed".into(), Some("npm install -g @openai/codex".into()));
        choose_model(&mut a, "gpt-5.5", &t);
        assert_eq!(a.message.0, "can't switch to Codex: not installed · install: npm install -g @openai/codex");
        let codex = a.engines.infos.iter_mut().find(|e| e.id == "codex").unwrap();
        (codex.installed, codex.connected) = (true, true);
        a.agents.races.push(crate::race::Race {
            task: "x".into(),
            started: std::time::Instant::now(),
            racers: vec![0],
            labels: vec![],
            ran: vec![],
            finished: vec![],
            announced: false,
            waiting: vec![],
            prompt: None,
        });
        choose_model(&mut a, "gpt-5.5", &t);
        assert_eq!(a.message.0, "a racer keeps its engine; :new starts an agent on another one");
        assert_eq!((a.sessions[0].engine.as_str(), a.sessions[0].requested_model.as_deref()), ("claude", Some("claude-opus-5-5")), "nothing changed");
        choose_model(&mut a, "claude-sonnet-5-5", &t);
        assert_eq!(a.sessions[0].requested_model.as_deref(), Some("claude-sonnet-5-5"), "a racer still changes its model on its own engine");
    }

    #[test]
    fn to_pi_a_new_conversation_and_back() {
        let mut a = switch_app();
        badge(&mut a, "codex", "gpt-5.5");
        a.engines.env = crate::engines::detect::tests::env(&[("PATH", "")]);
        let t = target(&a);
        choose_model(&mut a, "fake/test", &t);
        let s = &a.sessions[0];
        assert_eq!((s.engine.as_str(), s.requested_model.as_deref(), s.pi_fresh), ("pi", Some("fake/test"), true));
        assert_eq!(a.message.0, "'badge' now runs fake/test (Test Model, pi)");
        a.sessions[0].handoff = None;
        a.sessions[0].items.push(Item::Text("on it".into()));
        let t = target(&a);
        choose_model(&mut a, "claude-opus-5-5", &t);
        let s = &a.sessions[0];
        assert_eq!((s.engine.as_str(), s.pi.is_none(), s.pi_fresh), ("claude", true, false));
        assert!(s.handoff.as_deref().unwrap().contains("ran on pi (fake/test)"));
        assert!(s.handoff.as_deref().unwrap().contains("<assistant>on it</assistant>\n\nContinue from here"));
    }

    #[test]
    fn a_record_with_a_handoff_still_to_send_reopens_with_it() {
        let mut a = switch_app();
        // pi is here (else a pi agent moves to another engine as it reopens)
        let dir = std::env::temp_dir().join(format!("vigent-reopen-pi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pi"), "#!/bin/sh\n").unwrap();
        std::process::Command::new("chmod").arg("755").arg(dir.join("pi")).status().unwrap();
        a.engines.env = crate::engines::detect::tests::env(&[("PATH", &dir.to_string_lossy())]);
        let json = serde_json::json!({"name": "badge", "model": "", "createdAt": "2026-11-14T00:00:00.000Z", "engine": "codex", "requestedModel": "gpt-5.5", "handoff": HANDOFF, "modelName": "GPT-5.5", "theirs": 1});
        let meta = crate::records::AgentMeta::from_json(&json).unwrap();
        assert_eq!(meta.to_json(), json, "written back as read, with keys this build doesn't know");
        let s = a.reopen(&meta, "fix/badge", std::path::Path::new("/nope"), None);
        assert_eq!((s.engine.as_str(), s.handoff.as_deref(), s.model_name.as_deref(), s.pi_fresh), ("codex", Some(HANDOFF), Some("GPT-5.5"), false));
        let pi = crate::records::AgentMeta { engine: Some("pi".into()), model: "fake/test".into(), ..meta.clone() };
        assert!(a.reopen(&pi, "fix/badge", std::path::Path::new("/nope"), None).pi_fresh, "a new pi conversation, which the handoff starts");
        // a record from before: no handoff
        let old = crate::records::AgentMeta::from_json(&serde_json::json!({"name": "old", "model": "m", "createdAt": ""})).unwrap();
        assert_eq!((old.handoff, old.model_name), (None, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_first_prompt_that_cant_start_says_why() {
        let mut a = app();
        a.engines.infos = vec![EngineInfo { install: Some("npm install -g @anthropic-ai/claude-code".into()), ..info("claude", false, false, None) }];
        a.engines.preferred = "claude";
        assert_eq!(cant_start(&a, None).unwrap(), "Claude Code isn't installed: npm install -g @anthropic-ai/claude-code · :checkhealth");
        a.engines.preferred = "auto";
        a.engines.infos = vec![
            info("pi", true, false, Some("export ANTHROPIC_API_KEY=… (or OPENAI_API_KEY, GEMINI_API_KEY)")),
            info("claude", true, false, Some("claude auth login")),
            info("codex", true, false, Some("codex login")),
            info("opencode", false, false, None),
        ];
        assert_eq!(cant_start(&a, None).unwrap(), "no engine is signed in: run claude auth login or codex login, or for pi export ANTHROPIC_API_KEY=… (or OPENAI_API_KEY, GEMINI_API_KEY) · :checkhealth");
        assert_eq!(cant_start(&a, Some("claude")), None, "an engine you picked yourself still starts");
    }

    #[test]
    fn a_refused_tool_is_asked_about_when_ask_is_on() {
        let mut a = app();
        let mut s = Session::new(0, "pr", "claude", "/nope".into());
        s.launch.ask = true;
        s.state = crate::session::State::Running;
        s.apply(vec![super::super::Ev::Denied(vec![super::super::permissions::denial("Bash", serde_json::json!({"command": "gh pr view 12"}))])], true);
        assert!(s.ask.is_some());
        a.sessions.push(s);
        a.focus = 0;
        ask_to_allow(&mut a, 0);
        let p = a.windows.picker.as_ref().unwrap();
        assert_eq!(p.title, "'pr' was blocked · gh pr view 12");
        let rows: String = p.lines(100, &a.theme, "unicode").iter().flat_map(|l| l.spans.iter().map(|s| s.content.to_string())).collect();
        assert!(rows.contains("allow once") && rows.contains("always allow") && rows.contains("saved to [engines.claude] allow"));
        // no: left blocked
        a.sessions[0].state = crate::session::State::Waiting;
        a.key(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('q'), crossterm::event::KeyModifiers::NONE));
        assert_eq!(a.sessions[0].items.last(), Some(&crate::session::Item::Info("left blocked".into())));
    }

    #[test]
    fn all_models_in_one_list() {
        let env = crate::engines::detect::tests::env(&[("PATH", "")]);
        let l = all_model_choices("claude", &["claude".into(), "codex".into()], &env);
        assert_eq!(l[0].value, "default");
        assert_eq!(l[0].note, None);
        assert_eq!(l.iter().filter(|c| c.value.contains("default")).count(), 1, "one Default");
        assert_eq!(l.len(), 5, "no Codex models without Codex installed");
    }
}
