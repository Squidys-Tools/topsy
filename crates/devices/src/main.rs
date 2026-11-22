//! `cargo devices`: the headset catalog's tool, for contributors and the coding agents topsy's
//! *Add my headset…* starts (`.github/devices/PLAYBOOK.md`). It reads `devices/catalog.toml`
//! from this checkout and the `context.json` topsy writes, and builds without topsy.
//!
//!   cargo devices match <context.json>
//!   cargo devices add <context.json> --device <name> --slug <slug> [--name <model>] [--form <form>]
//!   cargo devices check
//!   cargo devices render <folder>
//!   cargo devices generic

mod render;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use model::headset::catalog::{Catalog, Rule};
use model::headset::{Dump, Dumped, Form};

const USAGE: &str = "usage:
  cargo devices match <context.json>
      which catalog entry each device in topsy's context.json matches, and why
  cargo devices add <context.json> --device <name> --slug <slug> [--name <model>] [--form <form>]
      add the device's product ids to devices/catalog.toml: to the entry with that slug, or as
      a new model, which needs its name; form: stem-buds | buds | over-ear | speaker (default:
      the shape topsy guesses)
  cargo devices check
      check devices/catalog.toml, as the tests do
  cargo devices render <folder>
      render <folder>/model.glb with Blender into <folder>/spin.png
  cargo devices generic
      rebuild the generic shapes (devices/generic/build.py) and render each";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("match") if args.len() == 2 => matches(Path::new(&args[1])),
        Some("add") if args.len() >= 2 => add(Path::new(&args[1]), &args[2..]),
        Some("check") if args.len() == 1 => check(),
        Some("render") if args.len() == 2 => render::render(Path::new(&args[1])),
        Some("generic") if args.len() == 1 => render::generic(),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn catalog_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../devices/catalog.toml")
}

fn read_catalog() -> Result<(String, Catalog), String> {
    let path = catalog_path();
    let text =
        std::fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    let catalog = Catalog::parse(&text).map_err(|err| format!("devices/catalog.toml: {err}"))?;
    Ok((text, catalog))
}

fn read_dump(path: &Path) -> Result<Dump, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))
}

fn matches(context: &Path) -> Result<(), String> {
    let (_, catalog) = read_catalog()?;
    let dump = read_dump(context)?;
    if dump.devices.is_empty() {
        println!("no Bluetooth audio devices in {}", context.display());
    }
    for device in &dump.devices {
        let identity = &device.identity;
        let look = catalog.identify(identity);
        let ids = match identity.product {
            Some(product) => Rule::of(product).to_toml(),
            None => "no product ids".to_owned(),
        };
        let state = if device.present {
            "connected"
        } else {
            "paired"
        };
        let verdict = match look.entry {
            Some(entry) if identity.product.is_some_and(|p| entry.matches_product(p)) => {
                format!("{} by its product ids", entry.slug)
            }
            Some(entry) => format!("{} by its name only", entry.slug),
            None => format!("nothing; drawn as {}", look.form.slug()),
        };
        println!("{} ({state}, {ids}): matches {verdict}", identity.name);
    }
    Ok(())
}

fn add(context: &Path, flags: &[String]) -> Result<(), String> {
    let flag = |name: &str| {
        flags
            .iter()
            .position(|f| f == name)
            .and_then(|at| flags.get(at + 1))
            .map(String::as_str)
    };
    let (Some(wanted), Some(slug)) = (flag("--device"), flag("--slug")) else {
        return Err(USAGE.to_owned());
    };
    let dump = read_dump(context)?;
    let device = find(&dump.devices, wanted)?;
    let Some(product) = device.identity.product else {
        return Err(format!(
            "{} has no product ids: add an entry matched by {{ name = \"...\" }} to devices/catalog.toml by hand",
            device.identity.name
        ));
    };
    let (text, catalog) = read_catalog()?;
    if let Some(entry) = catalog.devices.iter().find(|e| e.matches_product(product)) {
        return Err(format!("{} already matches these product ids", entry.slug));
    }
    let rule = Rule::of(product).to_toml();
    let (updated, added) = if catalog.devices.iter().any(|e| e.slug == slug) {
        let at = text
            .find(&format!("slug = {slug:?}"))
            .and_then(|at| Some(at + text[at..].find("match = [")? + "match = [".len()))
            .ok_or_else(|| format!("{slug}: no `match = [` line to add to"))?;
        let updated = format!("{}{rule}, {}", &text[..at], &text[at..]);
        (updated, format!("{rule} to {slug}"))
    } else {
        let Some(name) = flag("--name") else {
            return Err(format!("{slug} is a new model: give it a --name"));
        };
        let form = match flag("--form") {
            Some(form) => Form::ALL
                .into_iter()
                .find(|f| f.slug() == form)
                .ok_or_else(|| {
                    format!("no shape {form:?}: stem-buds, buds, over-ear or speaker")
                })?,
            None => catalog.identify(&device.identity).form,
        };
        let entry = format!(
            "[[device]]\nslug = {slug:?}\nname = {name:?}\nform = {:?}\nmatch = [{rule}]\n",
            form.slug(),
        );
        (
            format!("{}\n\n{entry}", text.trim_end()),
            format!("\n{entry}"),
        )
    };
    let problems = Catalog::parse(&updated)
        .map_err(|err| format!("the new entry doesn't parse: {err}"))?
        .problems();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    std::fs::write(catalog_path(), updated).map_err(|err| err.to_string())?;
    println!("added to devices/catalog.toml: {added}");
    Ok(())
}

/// The device named `wanted`: by its whole name, else the one name that contains it.
fn find<'a>(devices: &'a [Dumped], wanted: &str) -> Result<&'a Dumped, String> {
    let name = |d: &Dumped| d.identity.name.to_lowercase();
    let wanted = wanted.to_lowercase();
    if let Some(exact) = devices.iter().find(|d| name(d) == wanted) {
        return Ok(exact);
    }
    let mut partial = devices.iter().filter(|d| name(d).contains(&wanted));
    match (partial.next(), partial.next()) {
        (Some(one), None) => Ok(one),
        (Some(_), Some(_)) => Err(format!("more than one device is called {wanted:?}")),
        (None, _) => Err(format!("no device called {wanted:?} in context.json")),
    }
}

fn check() -> Result<(), String> {
    let (_, catalog) = read_catalog()?;
    let problems = catalog.problems();
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    println!(
        "devices/catalog.toml: {} models, no problems",
        catalog.devices.len()
    );
    Ok(())
}
