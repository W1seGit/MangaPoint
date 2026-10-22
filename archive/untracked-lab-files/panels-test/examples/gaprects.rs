use converter::decode::decode_to_gray;
use converter::process::DEFAULT_TRIM_SENSITIVITY;
use converter::xtc::ReadDirection;
use converter::find_panels;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = if a.iter().any(|x| x == "--rtl") { ReadDirection::RightToLeft } else { ReadDirection::LeftToRight };
    let mut files: Vec<_> = std::fs::read_dir(&a[0]).unwrap().filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "jpg" || e == "png")).collect();
    files.sort();
    for f in files {
        let g = decode_to_gray(&std::fs::read(&f).unwrap()).unwrap();
        let t = std::time::Instant::now();
        let p = find_panels(&g, dir, DEFAULT_TRIM_SENSITIVITY, 0);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let r: Vec<String> = p.iter().map(|r| format!("{},{},{},{}", r.x, r.y, r.w, r.h)).collect();
        println!("{}\t{:.1}\t{}", f.file_name().unwrap().to_string_lossy(), ms, r.join(";"));
    }
}
