mod app;
mod fonts;
mod highlight;
mod i18n;
mod model;
mod review;
#[cfg(test)]
mod schema;
mod schema_engine;
mod worker;

pub use app::run;
