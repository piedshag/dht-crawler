## DHT Crawler

DHT Crawler crawls a Distributed Hash Table (DHT) network to collect information about torrent infohashes.

It currently recursively sends find_node requests to nodes from an initial bootstrap list

# Usage

To start the DHT Crawler, run the following command with the -b flag followed by a list of nodes in the IP:PORT format, separated by commas:

```sh
dht-crawler -b 127.0.0.1:8000,192.168.1.1:8001
```