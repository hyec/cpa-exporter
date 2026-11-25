mod collector;
mod config;
mod metrics;
mod record;
mod resp;

use config::Config;
use metrics::Metrics;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "--help" | "-help" | "-h"))
    {
        print!("{}", Config::help());
        return Ok(());
    }
    let cfg = Config::parse(args, |key| std::env::var(key).ok())?;
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, stop.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, stop.clone())?;
    let address = if cfg.listen_address.starts_with(':') {
        format!("0.0.0.0{}", cfg.listen_address)
    } else {
        cfg.listen_address.clone()
    };
    let server = tiny_http::Server::http(&address)?;
    let metrics = Arc::new(Metrics::new(&cfg)?);
    let worker_metrics = metrics.clone();
    let worker_stop = stop.clone();
    let worker_cfg = cfg.clone();
    std::thread::Builder::new()
        .name("collector".into())
        .spawn(move || collector::run(worker_cfg, worker_metrics, worker_stop))?;
    eprintln!(
        "cpa-exporter listening on {address}{} in {} mode",
        cfg.metrics_path, cfg.mode
    );
    while !stop.load(Ordering::Relaxed) {
        let Some(request) = server.recv_timeout(Duration::from_millis(100))? else {
            continue;
        };
        let path = request.url().split('?').next().unwrap_or("");
        let response = if path == cfg.metrics_path {
            match metrics.render() {
                Ok(bytes) => tiny_http::Response::from_data(bytes).with_header(
                    tiny_http::Header::from_bytes(
                        "Content-Type",
                        "text/plain; version=0.0.4; charset=utf-8",
                    )
                    .unwrap(),
                ),
                Err(_) => tiny_http::Response::from_string("metrics encoding failed\n")
                    .with_status_code(500),
            }
        } else if path == "/-/healthy" {
            tiny_http::Response::from_string("ok\n")
        } else {
            tiny_http::Response::from_string("not found\n").with_status_code(404)
        };
        let _ = request.respond(response);
    }
    Ok(())
}
fn main() {
    if let Err(err) = run() {
        eprintln!("cpa-exporter: {err}");
        std::process::exit(1);
    }
}
