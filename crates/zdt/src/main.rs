//! The entry point.

use std::path::PathBuf;

use zdt_core::Project;
use zgui::prelude::*;
use zgui::view;

use zdt::app::RootProps;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ZDT_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,zdt=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let (project, files) = arguments();

    // Before the first window and before any background work: the runtime has to be the one
    // behind `background` and `blocking` from the start, and it is what lets a language server's
    // socket be awaited on the interface thread.
    let _tokio = zgui::tokio::install()?;

    let title = format!("{} \u{2014} zdt", project.name());

    app()
        .with_application_id("dev.zdt.Editor")
        .with_title(title)
        .with_size(1280.0, 800.0)
        .with_min_size(480.0, 320.0)
        // The two that make the window this application's to draw. `assets/css/frame.css` draws
        // what the desktop dropped.
        .with_decorations(Decorations::None)
        .with_transparent(true)
        .with_stylesheet(zdt::assets::sheet())
        .run(move || {
            view! { Root(project = project.clone(), files = files.clone()) }
        })?;

    Ok(())
}

/// What was asked for on the command line: a project to work in, and files to open in it.
///
/// A directory argument is the project outright. With only files, the project is discovered from
/// the first of them. Opening one file in a repository puts the whole repository in the tree,
/// which is what somebody means by opening a file in a project.
fn arguments() -> (Project, Vec<PathBuf>) {
    let mut directory: Option<PathBuf> = None;
    let mut files: Vec<PathBuf> = Vec::new();

    for argument in std::env::args_os().skip(1) {
        let path = PathBuf::from(argument);
        if path.is_dir() {
            directory.get_or_insert(path);
        } else {
            files.push(path);
        }
    }

    let real = |path: PathBuf| std::fs::canonicalize(&path).unwrap_or(path);
    let files: Vec<PathBuf> = files.into_iter().map(real).collect();

    let project = match directory {
        Some(directory) => Project::at(real(directory)),
        None => Project::discover(&files.first().cloned().unwrap_or_else(|| real(".".into()))),
    };

    (project, files)
}
