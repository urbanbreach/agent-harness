fn main() {
    let parts = vec![(0_u64, String::new())];
    let unzipped: (Vec<_>, Vec<_>) = parts.into_iter().unzip();
    let parts = vec![(0_u64, String::new())];
    let mut sized = (Vec::with_capacity(parts.len()), Vec::with_capacity(parts.len()));
    sized.extend(parts);
    println!("unzip capacities: {} {}", unzipped.0.capacity(), unzipped.1.capacity());
    println!("sized capacities: {} {}", sized.0.capacity(), sized.1.capacity());
}
