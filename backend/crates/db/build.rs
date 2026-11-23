// `sqlx::migrate!` embeds the migrations when this crate compiles; without this, adding a
// migration file doesn't rebuild it, and tests and binaries run with the old set.
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
