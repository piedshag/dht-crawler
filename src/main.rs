use std::env;

mod torrent;
mod tracker;

fn main () {
    let args: Vec<String> = env::args().collect();

    let magnet = &args[1];
    let mut torrent = torrent::Torrent::new();
    if Ok(()) == torrent.from_magnet(magnet) {
        println!("{:?}", torrent)
    }

    let trackers = torrent.get_trackers().unwrap();
    let first_tracker = tracker::Tracker::new(&trackers[0]);
    first_tracker.connect();
}