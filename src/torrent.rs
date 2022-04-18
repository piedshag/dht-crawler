const MAGNET_PREFIX: &str = "magnet:?";

#[derive(Debug)]
pub struct Torrent {
    infohash: Option<String>,
    name: Option<String>,
    trackers: Option<Vec<String>>,
}

impl Torrent {
    pub fn new() -> Self {
        return Torrent {
            infohash: None,
            name: None,
            trackers: Some(Vec::new()),
        }
    }

    pub fn from_magnet(&mut self, magnet: &str) -> Result<(), &str> {
        if !magnet.starts_with(MAGNET_PREFIX) {
            return Err("Invalid magnet")
        }

        let content = magnet.strip_prefix(MAGNET_PREFIX);
        if let Some(data) = content {
            for pair in data.split("&").into_iter().map(|param| param.split("=")) {
                let p: Vec<&str> = pair.collect();
                match p[0] {
                    "tr" => self.add_tracker(p[1]),
                    "xt" => self.add_infohash(p[1]),
                    "dn" => self.add_name(p[1]),
                    _ => (),
                }
            }
        }

        Ok(())
    }

    pub fn get_trackers(&self) -> Result<&Vec<String>, &str> {
        match &self.trackers {
            Some(trackers) => return Ok(trackers),
            None => return Err("No trackers supplied")
        }
    }

    fn add_infohash(&mut self, hash: &str) {
        let data: Vec<&str> = hash.split(":").collect();
        match data[1] {
            "btih" => self.infohash = Some(data[2].to_string()),
            _ => (),
        }
    }

    fn add_name(&mut self, name: &str) {
        self.name = Some(name.to_string())
    }

    fn add_tracker(&mut self, tracker: &str) {
        if let Some(ref mut tracker_list) = self.trackers {
            tracker_list.push(tracker.to_string())
        }
    }
}
