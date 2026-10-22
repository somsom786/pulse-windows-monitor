use pulse_lib::scanner::*;
use std::sync::{atomic::AtomicBool, Arc};
fn main() {
    let root = std::env::args().nth(1).unwrap_or("C:\\".into());
    let t = std::time::Instant::now();
    let tree = scan(&root, Arc::new(AtomicBool::new(false)), |p| { eprint!("\r{} files {} dirs {:.1}GB   ", p.files, p.dirs, p.bytes as f64/1e9); });
    eprintln!();
    let m = &tree.meta;
    println!("scan {:?}: files={} dirs={} logical={:.1}GB alloc={:.1}GB errors={} hardlink_saved={:.1}GB ({} files) volume used={:.1}GB",
        t.elapsed(), m.files, m.dirs, m.total_logical as f64/1e9, m.total_alloc as f64/1e9, m.errors, m.hardlink_saved as f64/1e9, m.hardlink_files, (m.volume_total-m.volume_free) as f64/1e9);
    let mut top: Vec<u32> = tree.children(0).collect();
    top.sort_by_key(|&c| std::cmp::Reverse(tree.nodes[c as usize].alloc));
    for c in top.iter().take(10) { println!("{:>10.2}GB {}", tree.nodes[*c as usize].alloc as f64/1e9, tree.name(*c)); }
    let t = std::time::Instant::now();
    save_tree(&tree).unwrap();
    println!("saved in {:?}, nodes={}", t.elapsed(), tree.nodes.len());
    let t = std::time::Instant::now();
    let l = load_tree(&tree.meta.id).unwrap();
    println!("loaded in {:?}, nodes={}", t.elapsed(), l.nodes.len());
}
