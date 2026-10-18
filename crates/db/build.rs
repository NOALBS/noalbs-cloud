// `sqlx::migrate!` embeds the migrations at compile time; rebuild when one is
// added or changed, or a new migration could be silently left out.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
