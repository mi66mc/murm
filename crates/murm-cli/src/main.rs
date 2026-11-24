use clap::{CommandFactory, Parser};
use murm_cli::{
    ClientService,
    commands::{Cli, interactive},
    default_data_dir,
};
use std::process::ExitCode;
#[tokio::main(worker_threads = 2)]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            print!("{error}");
            return ExitCode::SUCCESS;
        }
        Err(_) => {
            eprintln!(
                "Invalid arguments. Run murm-cli --help. Private keys are accepted only through files, stdin or a hidden form."
            );
            return ExitCode::from(2);
        }
    };
    if cli.command.is_none() && !interactive() {
        let _ = Cli::command().print_help();
        return ExitCode::SUCCESS;
    }
    let result = async {
        let app = if let Some(path) = &cli.db {
            ClientService::open_db_with_capacity(
                path,
                cli.max_db_bytes
                    .unwrap_or(ClientService::DEFAULT_DATABASE_BYTES),
            )?
        } else {
            ClientService::open_with_capacity(
                &cli.data_dir
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(default_data_dir)?,
                cli.max_db_bytes
                    .unwrap_or(ClientService::DEFAULT_DATABASE_BYTES),
            )?
        };
        if cli.command.is_none() {
            return murm_cli::tui::run(app).await;
        }
        let value = murm_cli::commands::run(&app, cli).await?;
        println!(
            "{}",
            serde_json::to_string_pretty(&value)
                .map_err(|_| murm_cli::ClientError::InvalidInput)?
        );
        Ok::<_, murm_cli::ClientError>(())
    }
    .await;
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
