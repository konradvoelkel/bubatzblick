//! `bubatz` -- die ganze Pipeline als ein Binary: `extract`, `clip`, `zones`,
//! `tiles`, `pedestrian`, `report`, `update-pbf`, `verify`, dazu `serve` als
//! Vorschau- und Kachelserver (serve.rs). Jede Stufe schreibt `*_summary.json`.

mod assemble;
mod bstore;
mod clip;
mod config;
#[cfg(test)]
mod contract_tests;
mod extract;
mod geoparquet;
mod geos;
mod grid;
mod obstacles;
mod parquet_io;
mod pbf;
mod pedestrian;
mod proj;
mod report;
mod sample;
mod serve;
mod simplify;
mod tags;
mod tiles;
mod update_pbf;
mod verify;
mod wkb;
mod zones;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bubatz",
    version,
    about = "bubatzblick: Verbotszonen aus OSM, ohne Python"
)]
struct Cli {
    /// Pfad zu config/pipeline.toml
    #[arg(long, global = true, default_value = "config/pipeline.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// PBF -> protected.parquet, buildings.parquet (EPSG:25832)
    Extract(extract::ExtractArgs),
    /// protected.parquet auf eine Verwaltungsgrenze + Marge filtern
    Clip(clip::ClipArgs),
    /// Sichtfeld + Ringzerlegung je Verbotsobjekt (Zellendateien nach --out-dir)
    Zones(zones::ZonesArgs),
    /// Zonen-Zellen -> zonen.pmtiles (Zellen-Cache, tippecanoe je Zoomband, tile-join)
    Tiles(tiles::TilesArgs),
    /// Kennzahlen je Kategorie, groesste Zonen, Diff gegen einen Vorlauf
    Report(report::ReportArgs),
    /// Pruefungen (identical, anchor; README.md "Pruefungen je Lauf")
    Verify(verify::VerifyArgs),
    /// Vorschau-Seite und Kacheln aus zonen.pmtiles-Archiven per HTTP (ersetzt pmtiles serve + http.server)
    Serve(serve::ServeArgs),
    /// PBF per OSM-Replikationsdiffs aktualisieren (osmium apply-changes)
    UpdatePbf(update_pbf::UpdateArgs),
    /// Fussgaengerzonen (§ 5 Abs. 2 Nr. 5): highway=pedestrian -> eigenes Kachelarchiv, ohne Puffer
    Pedestrian(pedestrian::PedestrianArgs),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let cli = Cli::parse();
    match cli.command {
        Command::Extract(args) => extract::run(&cli.config, args),
        Command::Clip(args) => clip::run(&cli.config, args),
        Command::Zones(args) => {
            let cfg = config::load(&cli.config)?;
            zones::run(&cfg, args)
        }
        Command::Report(args) => report::run(args),
        Command::Verify(args) => verify::run(&cli.config, args),
        Command::UpdatePbf(args) => update_pbf::run(args),
        Command::Serve(args) => serve::run(args),
        Command::Pedestrian(args) => pedestrian::run(args),
        Command::Tiles(args) => {
            let cfg = config::load(&cli.config)?;
            tiles::run(&cfg, args)
        }
    }
}
