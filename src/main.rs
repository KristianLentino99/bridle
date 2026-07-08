use bridle::cli::{Cli, Commands};
use clap::Parser;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => bridle::commands::init::run(),
        Commands::Discover => bridle::commands::discover::run(),
        Commands::Sync {
            watch,
            force,
            no_skills,
        } => bridle::commands::sync::run(watch, force, no_skills),
        Commands::Status => bridle::commands::status::run(),
        Commands::Add {
            name,
            command,
            args,
            url,
            env,
        } => bridle::commands::add::run(name, command, args, url, env),
        Commands::Remove { args } => bridle::commands::remove::run(args),
        Commands::List => bridle::commands::list::run(),
        Commands::Import {
            what,
            harness,
            all,
            force,
            link,
            update,
            source,
        } => bridle::commands::import::run(what, harness, all, force, link, update, source),
        Commands::Profile { command } => bridle::commands::profile::run(command),
    }
}
