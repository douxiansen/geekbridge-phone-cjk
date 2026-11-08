// SPDX-License-Identifier: GPL-3.0-or-later
//! `geekbridge-phone`: the desktop app.

use phone_app::instance::Request;
use phone_app::model::tab::Tab;
use phone_app::RunOptions;
use phone_core::settings::ThemePref;

const HELP: &str = "GeekBridge Phone: calls and texts from your computer, over Bluetooth

USAGE: geekbridge-phone [OPTIONS]

OPTIONS:
    --open <NUMBER>   Show the conversation with NUMBER (in the running app if there is one)
    --minimised       Start minimised (alias: --background)
    tel:NUMBER        Show the keypad with NUMBER typed in (the running app if there is one)
    --tab <NAME>      Start on keypad, contacts, recents, messages or settings
    --theme <NAME>    system, light or dark for this run
    --demo            Use a fake phone with invented data (for demos and screenshots)
    --scene <NAME>    With --demo: ring, ring-unknown, active, dialing or text
    --size <WxH>      Initial window size, for example 940x660
    --zoom <FACTOR>   Scale the interface (1.4 shows the 720 px layout in a 1000 px window)
    --tour            With --demo: visit every screen in turn (for screenshots)
    --real-alerts     With --demo: show alerts on the desktop instead of logging them
    -V, --version     Print the version
    -h, --help        Print this help";

/// What the command line asked for.
#[derive(Debug, Default, PartialEq)]
struct Cli {
    opts: Parsed,
    exit_text: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
struct Parsed {
    demo: bool,
    open: Option<String>,
    dial: Option<String>,
    minimised: bool,
    tab: Option<Tab>,
    theme: Option<ThemePref>,
    scene: Option<String>,
    size: Option<[f32; 2]>,
    zoom: Option<f32>,
    real_alerts: bool,
    tour: bool,
}

fn parse_size(s: &str) -> Option<[f32; 2]> {
    let (w, h) = s.split_once(['x', 'X'])?;
    let (w, h): (f32, f32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    ((200.0..=8000.0).contains(&w) && (200.0..=8000.0).contains(&h)).then_some([w, h])
}

fn parse_theme(s: &str) -> Option<ThemePref> {
    match s.to_ascii_lowercase().as_str() {
        "system" => Some(ThemePref::System),
        "light" => Some(ThemePref::Light),
        "dark" => Some(ThemePref::Dark),
        _ => None,
    }
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut args = args.into_iter();
    let value = |flag: &str, args: &mut dyn Iterator<Item = String>| args.next().ok_or_else(|| format!("{flag} needs a value"));
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => cli.exit_text = Some(HELP.to_string()),
            "-V" | "--version" => cli.exit_text = Some(format!("geekbridge-phone {}", env!("CARGO_PKG_VERSION"))),
            "--demo" => cli.opts.demo = true,
            "--minimised" | "--minimized" | "--background" => cli.opts.minimised = true,
            "--tour" => cli.opts.tour = true,
            "--real-alerts" => cli.opts.real_alerts = true,
            "--open" => cli.opts.open = Some(value("--open", &mut args)?),
            "--tab" => {
                let v = value("--tab", &mut args)?;
                cli.opts.tab = Some(Tab::parse(&v).ok_or_else(|| format!("unknown tab {v:?}"))?);
            }
            "--theme" => {
                let v = value("--theme", &mut args)?;
                cli.opts.theme = Some(parse_theme(&v).ok_or_else(|| format!("unknown theme {v:?}"))?);
            }
            "--zoom" => {
                let v = value("--zoom", &mut args)?;
                cli.opts.zoom = Some(v.parse().map_err(|_| format!("bad zoom {v:?}"))?);
            }
            "--scene" => cli.opts.scene = Some(value("--scene", &mut args)?),
            "--size" => {
                let v = value("--size", &mut args)?;
                cli.opts.size = Some(parse_size(&v).ok_or_else(|| format!("bad size {v:?}, expected WIDTHxHEIGHT"))?);
            }
            other if other.starts_with("tel:") || other.starts_with("callto:") || other.starts_with("TEL:") => {
                cli.opts.dial = Some(phone_app::model::keypad::parse_tel_uri(other).ok_or_else(|| format!("no phone number in {other:?}"))?);
            }
            other => return Err(format!("unknown option {other:?}; try --help")),
        }
    }
    if (cli.opts.scene.is_some() || cli.opts.tour) && !cli.opts.demo {
        return Err("--scene and --tour only work with --demo".into());
    }
    Ok(cli)
}

fn main() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).try_init();
    let cli = match parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("geekbridge-phone: {e}");
            std::process::exit(2);
        }
    };
    if let Some(text) = cli.exit_text {
        println!("{text}");
        return;
    }
    let p = cli.opts;
    let mut opts = RunOptions {
        demo: p.demo,
        request: p.open.map(Request::Open).or(p.dial.map(Request::Dial)),
        tab: p.tab,
        theme: p.theme,
        size: p.size,
        zoom: p.zoom,
        demo_real_alerts: p.real_alerts,
        minimised: p.minimised,
        ..RunOptions::default()
    };
    #[cfg(feature = "demo")]
    {
        opts.tour = p.tour;
    }
    #[cfg(feature = "demo")]
    if let Some(scene) = &p.scene {
        match phone_app::demo::Scene::parse(scene) {
            Some(s) => opts.scene = Some(s),
            None => {
                eprintln!("geekbridge-phone: unknown scene {scene:?}");
                std::process::exit(2);
            }
        }
    }
    #[cfg(not(feature = "demo"))]
    let _ = &mut opts;
    if let Err(e) = phone_app::run(opts) {
        eprintln!("geekbridge-phone: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Cli, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_is_the_live_app() {
        let cli = args(&[]).unwrap();
        assert_eq!(cli.opts, Parsed::default());
        assert!(cli.exit_text.is_none());
    }

    #[test]
    fn flags_and_values_parse() {
        let cli = args(&["--demo", "--tab", "Messages", "--theme", "dark", "--size", "720x520", "--scene", "ring", "--open", "+1555"]).unwrap();
        assert!(cli.opts.demo);
        assert_eq!(cli.opts.tab, Some(Tab::Messages));
        assert_eq!(cli.opts.theme, Some(ThemePref::Dark));
        assert_eq!(cli.opts.size, Some([720.0, 520.0]));
        assert_eq!(cli.opts.open.as_deref(), Some("+1555"));
    }

    #[test]
    fn a_tel_link_becomes_a_dial_request() {
        let cli = args(&["tel:+1-555-0100"]).unwrap();
        assert_eq!(cli.opts.dial.as_deref(), Some("+15550100"));
        assert!(args(&["tel:abc"]).is_err());
        assert!(args(&["--background"]).unwrap().opts.minimised);
    }

    #[test]
    fn mistakes_are_reported() {
        assert!(args(&["--tab"]).unwrap_err().contains("needs a value"));
        assert!(args(&["--tab", "nope"]).is_err());
        assert!(args(&["--size", "wide"]).is_err());
        assert!(args(&["--size", "10x10"]).is_err());
        assert!(args(&["--wat"]).is_err());
        assert!(args(&["--scene", "ring"]).unwrap_err().contains("--demo"));
    }

    #[test]
    fn help_and_version_ask_to_exit() {
        assert!(args(&["--help"]).unwrap().exit_text.unwrap().contains("USAGE"));
        assert!(args(&["-V"]).unwrap().exit_text.unwrap().starts_with("geekbridge-phone "));
    }
}
