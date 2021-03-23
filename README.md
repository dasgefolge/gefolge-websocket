Dieses Projekt ist ein [WebSocket](https://en.wikipedia.org/wiki/WebSocket) server für [die gefolge.org API](https://gefolge.org/api).

# Protokoll

Der server verwendet [async-proto 0.7](https://docs.rs/async-proto/0.7). Dementsprechend sind einzelne Pakete als binäre WebSocket-Nachrichten dargestellt.

## Verbindungsaufbau

1. Der client sendet seinen API key als [`String`](https://docs.rs/async-proto/0.7/async_proto/trait.Protocol.html#impl-Protocol-for-String).
2. Der client sendet ein byte, das den Zweck der Verbindung darstellt:
    * `0`: Rasende Roboter (siehe <https://github.com/dasgefolge/ricochet-robots>)
