extern crate clap;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Pathname of PCC file to input
    pccfile: String,

    /// Directory where PCC and LST files are found
    #[arg(short, long, default_value = ".")]
    datadir: String,
}

fn main() {
    let _args = Args::parse();

    println!("Hello, world!");
}
