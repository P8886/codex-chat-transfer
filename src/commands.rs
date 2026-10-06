use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use codex_chat_transfer::{
    bundle,
    importer::{self, ImportOptions},
    model, rpc,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Transfer Codex chats between computers with verified project membership"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Desktop,
    Diagnose,
    Serve {
        #[arg(long, default_value_t = 47831)]
        port: u16,
        #[arg(long)]
        no_open: bool,
    },
    List {
        #[arg(long)]
        home: Option<PathBuf>,
    },
    Export {
        #[arg(long)]
        home: Option<PathBuf>,
        #[arg(long,num_args=1..)]
        threads: Vec<String>,
        #[arg(long)]
        out: PathBuf,
    },
    Inspect {
        package: PathBuf,
    },
    Restore {
        backup: PathBuf,
        #[arg(long)]
        home: Option<PathBuf>,
    },
    Import {
        package: PathBuf,
        #[arg(long)]
        home: Option<PathBuf>,
        #[arg(long)]
        project_id: Option<String>,
        #[arg(long)]
        target_dir: Option<PathBuf>,
        #[arg(long)]
        project_name: Option<String>,
        #[arg(long)]
        replace: bool,
        #[arg(long)]
        codex: Option<PathBuf>,
        #[arg(long)]
        mappings: Option<PathBuf>,
    },
}

pub fn run(gui_default: bool) -> Result<()> {
    let command = Cli::parse().command.or(if gui_default {
        Some(Commands::Desktop)
    } else {
        None
    });
    let Some(command) = command else {
        Cli::command().print_help()?;
        return Ok(());
    };
    match command {
        Commands::Diagnose => {
            println!(
                "{}",
                serde_json::to_string_pretty(&codex_chat_transfer::runtime::inspect())?
            );
            Ok(())
        }
        Commands::Desktop => {
            #[cfg(windows)]
            {
                crate::desktop::run()
            }
            #[cfg(not(windows))]
            {
                crate::web::serve(47831, false)
            }
        }
        Commands::Serve { port, no_open } => crate::web::serve(port, no_open),
        Commands::List { home } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&model::scan(
                    &home.unwrap_or_else(model::default_home)
                )?)?
            );
            Ok(())
        }
        Commands::Export { home, threads, out } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&bundle::export(
                    &home.unwrap_or_else(model::default_home),
                    &threads,
                    &out
                )?)?
            );
            Ok(())
        }
        Commands::Inspect { package } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&bundle::load(&package)?.manifest)?
            );
            Ok(())
        }
        Commands::Restore { backup, home } => {
            importer::restore_backup(&home.unwrap_or_else(model::default_home), &backup)?;
            println!("已恢复备份：{}", backup.display());
            Ok(())
        }
        Commands::Import {
            package,
            home,
            project_id,
            target_dir,
            project_name,
            replace,
            codex,
            mappings,
        } => {
            let home = home.unwrap_or_else(model::default_home);
            let codex_binary = codex
                .map(Ok)
                .unwrap_or_else(rpc::detect_binary)
                .context("请使用 --codex 指定 Codex 可执行文件")?;
            let options = ImportOptions {
                home,
                package,
                project_id,
                target_dir,
                project_name,
                replace,
                codex_binary: Some(codex_binary),
                project_mappings: mappings
                    .map(|path| -> Result<_> { Ok(serde_json::from_slice(&std::fs::read(path)?)?) })
                    .transpose()?,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&importer::import(&options)?)?
            );
            Ok(())
        }
    }
}
