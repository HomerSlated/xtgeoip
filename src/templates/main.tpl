/// Design template for main.rs
fn main() -> Result<()> {
    // Parse CLI arguments early
    let cli = Cli::try_parse().map_err(|e| {
        eprintln!("CLI parsing error: {}", e);
        e
    })?;

    // Central runner for all commands
    run(cli)?;
    
    Ok(())
}

/// Run the selected command and propagate all errors as Result<()>
fn run(cli: Cli) -> Result<()> {
    // Load configuration (may fail)
    let cfg = load_config().map_err(|e| {
        eprintln!("Fatal: Failed to load config: {}", e);
        e
    })?;

    // Initialize logging if configured
    if let Some(log_file) = cfg.logging.as_ref().map(|l| l.log_file.as_str()) {
        init_logger(log_file)?;
    }

    // Enforce top-level flags rules
    enforce_flag_rules(&cli)?;

    // Dispatch commands
    match cli.command {
        Some(Commands::Conf { default, show, edit }) => {
            // Same pattern as others: return Result
            let action = if default {
                ConfAction::Default
            } else if show {
                ConfAction::Show
            } else {
                ConfAction::Edit
            };
            run_conf(action)?; // propagate any error
        }

        Some(Commands::Run { prune, legacy }) => {
            let (temp_dir, version) = fetch(&cfg, FetchMode::Remote)?;
            warn_legacy_mode(legacy);
            build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version, legacy)?;
            if prune {
                prune_archives(&cfg, true, false)?;
            }
        }

        Some(Commands::Build { backup, clean, force, legacy }) => {
            let (temp_dir, version) = fetch(&cfg, FetchMode::Local)?;
            warn_legacy_mode(legacy);
            build(temp_dir.path(), Path::new(&cfg.paths.output_dir), &version, legacy)?;
            if backup {
                backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), force)?;
            }
            if clean {
                delete(Path::new(&cfg.paths.output_dir), force)?;
            }
        }

        Some(Commands::Fetch { prune }) => {
            let _ = fetch(&cfg, FetchMode::Remote)?;
            if prune {
                prune_archives(&cfg, true, false)?;
            }
        }

        None => {
            if !(cli.backup || cli.clean || cli.prune) {
                Cli::command().print_help()?;
                println!();
                return Err(anyhow!("No command or top-level action specified"));
            }
        }
    }

    // Handle top-level actions outside subcommands
    if cli.backup {
        backup(Path::new(&cfg.paths.output_dir), Path::new(&cfg.paths.archive_dir), cli.force)?;
    }

    if cli.clean {
        delete(Path::new(&cfg.paths.output_dir), cli.force)?;
    }

    if cli.prune {
        prune_archives(&cfg, false, cli.backup)?;
    }

    Ok(())
}
