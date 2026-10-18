#[path = "/private/tmp/hawdb-wip-ready-20261015/phase-attribution-stage/retained_phase_profile.rs"]
mod phases;
fn main() {
 let timer = phases::Timer::start(phases::Phase::Pull);
 std::thread::spawn(move || drop(timer));
}
