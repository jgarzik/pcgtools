// Integration tests, compiled as one test binary so that all tests run
// in parallel.  Each module exercises the library through its public
// API (or the built CLI binary).

#![allow(dead_code)]

mod cli;
mod common;
mod corpus;
mod dataset;
mod gamemode;
mod oracle;
mod pcg;
mod reference;
mod repl;
mod rules;
mod sheet;
mod vars;
