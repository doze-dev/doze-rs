//! Prints a bbolt file's buckets and keys (values as text where they are, else their length): a look at what a Go
//! doze left, without Go. `cargo run -p doze-bolt --example bolt_dump -- <file>`.

#![allow(clippy::print_stdout)]

fn show(db: &doze_bolt::Db, path: &mut Vec<Vec<u8>>, depth: usize) {
    let p: Vec<&[u8]> = path.iter().map(Vec::as_slice).collect();
    for (k, item) in db.items(&p).unwrap_or_default() {
        let name = String::from_utf8_lossy(&k).escape_debug().to_string();
        match item {
            doze_bolt::Item::Bucket => {
                println!("{}[{name}]", "  ".repeat(depth));
                path.push(k);
                show(db, path, depth + 1);
                path.pop();
            }
            doze_bolt::Item::Value(v) => match std::str::from_utf8(&v) {
                Ok(s) if s.len() < 400 => println!("{}{name} = {s}", "  ".repeat(depth)),
                _ => println!("{}{name} = <{} bytes>", "  ".repeat(depth), v.len()),
            },
        }
    }
}

fn main() {
    for file in std::env::args().skip(1) {
        println!("== {file}");
        match doze_bolt::Db::open(std::path::Path::new(&file)) {
            Ok(db) => show(&db, &mut Vec::new(), 0),
            Err(e) => println!("{e}"),
        }
    }
}
