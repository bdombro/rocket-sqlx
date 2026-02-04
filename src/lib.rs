#[macro_use]
extern crate rocket;

pub mod db;
pub mod handlers;
pub mod util;

#[cfg(test)]
pub mod tests;
