use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use structureweaver::{
    analysis, export, ingest,
    llama::{LlamaClient, LlamaConfig},
    ui,
};

#[derive(Parser)]
#[command(
    name = "structureweaver",
    version,
    about = "Local workbook structure → ERD. Run without arguments for the native app."
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Open a workbook in the native app without starting model inference.
    Open { input: PathBuf },
    /// List models served by the local Llama app.
    Models {
        #[arg(long, default_value = "http://127.0.0.1:9931/v1")]
        base_url: String,
    },
    /// Inspect every worksheet and its cells without using the model.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Analyze all sheets through Llama and write JSON, Mermaid and SQL.
    Analyze {
        input: PathBuf,
        #[arg(long, default_value = "output")]
        output: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:9931/v1")]
        base_url: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = 180)]
        timeout: u64,
    },
}
fn main() -> Result<()> {
    match Args::parse().command {
        None => ui::run(),
        Some(Command::Open { input }) => ui::run_with_document(input),
        Some(Command::Models { base_url }) => {
            for model in LlamaClient::new(LlamaConfig {
                base_url,
                ..Default::default()
            })?
            .models()?
            {
                println!("{model}");
            }
            Ok(())
        }
        Some(Command::Inspect { input, output }) => {
            let document = ingest::read_document(&input)?;
            let json = serde_json::to_string_pretty(&document)?;
            if let Some(path) = output {
                std::fs::write(path, json)?;
            } else {
                println!("{json}");
            }
            Ok(())
        }
        Some(Command::Analyze {
            input,
            output,
            base_url,
            model,
            timeout,
        }) => {
            let document = ingest::read_document(&input)?;
            let result = analysis::analyze(
                document,
                LlamaConfig {
                    base_url,
                    model: model.unwrap_or_default(),
                    timeout_secs: timeout,
                },
                |message| eprintln!("{message}"),
                Arc::new(AtomicBool::new(false)),
            )?;
            export::write_outputs(&result, &output)?;
            println!(
                "{} sheets, {} tables, {} entities, {} relationship candidates → {}",
                result.pages.len(),
                result.tables.len(),
                result.schema.entities.len(),
                result.schema.relationships.len(),
                output.display()
            );
            for page in &result.pages {
                eprintln!("{}: {} — {}", page.page_name, page.state, page.message);
            }
            if result
                .pages
                .iter()
                .any(|p| matches!(p.state.as_str(), "partial" | "failed" | "unsupported"))
            {
                bail!("Analysis has unresolved pages; partial results were saved for review");
            }
            Ok(())
        }
    }
}
