use adaptive_ji::tuning::{
    Ratio,
    memory::{RelationMemory, fingerprint},
};

fn main() {
    let mut memory = RelationMemory::default();
    let pair = (4, 7);
    let relation = fingerprint(Ratio::new(6, 5).unwrap());
    println!("experimental harmonic relation memory demo");
    println!("pair E-G: relation=6/5 fingerprint={:?}", relation.0);
    println!(
        "memory starts empty; use --memory-mode relations in adaptive-ji to populate it during a phrase"
    );
    memory.clear();
    println!("pair key={pair:?} cleared");
}
