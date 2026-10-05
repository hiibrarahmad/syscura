//! Manual check: `cargo run -p syscura-online --example find -- "<query>" [wiki title]`
use syscura_online::{ImageFinder, query::PartQuery};

fn main() {
    let mut args = std::env::args().skip(1);
    let query = args.next().expect("query");
    let brand = query.split_whitespace().next().map(str::to_string);
    let part = PartQuery { key: "test".into(), kind: "test".into(), query, wiki_hint: args.next(), brand, ..Default::default() };
    match ImageFinder::new().find(&part) {
        Ok(img) => println!("{} {} bytes from {} ({})\n  image: {}", img.mime, img.bytes.len(), img.page_url, img.source, img.image_url),
        Err(e) => println!("failed: {e}"),
    }
}
