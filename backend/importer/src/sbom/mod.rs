use crate::progress::init_log_and_progress;
use sbom_walker::{
    retrieve::RetrievingVisitor,
    source::{DispatchSource, FileSource, HttpSource},
    validation::ValidationVisitor,
    walker::Walker,
};
use std::process::ExitCode;
use std::time::SystemTime;
use time::{Date, Month, UtcOffset};
use trustify_api::system::InnerSystem;
use trustify_common::config::Database;
use url::Url;
use walker_common::{fetcher::Fetcher, validate::ValidationOptions};

mod process;

#[derive(clap::Args, Debug)]
pub struct ImportSbomCommand {
    #[command(flatten)]
    pub database: Database,

    /// Source URL or path
    #[arg(short, long)]
    pub source: String,
}

impl ImportSbomCommand {
    pub async fn run(self) -> anyhow::Result<ExitCode> {
        let progress = init_log_and_progress()?;

        log::info!("Ingesting SBOMs");

        let system = InnerSystem::with_config(&self.database).await?;

        let source: DispatchSource = match Url::parse(&self.source) {
            Ok(url) => HttpSource::new(
                url,
                Fetcher::new(Default::default()).await?,
                Default::default(),
            )
            .into(),
            Err(_) => FileSource::new(&self.source, None)?.into(),
        };

        // process (called by validator)

        let process = process::ProcessVisitor { system };

        // validate (called by retriever)

        //  because we still have GPG v3 signatures
        let options = ValidationOptions::new().validation_date(SystemTime::from(
            Date::from_calendar_date(2007, Month::January, 1)?
                .midnight()
                .assume_offset(UtcOffset::UTC),
        ));

        let validation = ValidationVisitor::new(process).with_options(options);

        // retriever (called by filter)

        let visitor = RetrievingVisitor::new(source.clone(), validation);

        // walker

        Walker::new(source)
            .with_progress(progress)
            .walk(visitor)
            .await?;

        Ok(ExitCode::SUCCESS)
    }
}