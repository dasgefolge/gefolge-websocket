Dieses Projekt ist ein [WebSocket](https://en.wikipedia.org/wiki/WebSocket) server für [die gefolge.org API](https://gefolge.org/api).

# Protokoll

Der server verwendet [async-proto 0.7](https://docs.rs/async-proto/0.7). Dementsprechend sind einzelne Pakete als binäre WebSocket-Nachrichten dargestellt.

## Verbindungsaufbau

1. Der client sendet seinen API key als [`String`](https://docs.rs/async-proto/0.7/async_proto/trait.Protocol.html#impl-Protocol-for-String).
2. Der client sendet ein byte, das den Zweck der Verbindung darstellt:
    * `0`: Rasende Roboter (siehe <https://github.com/dasgefolge/ricochet-robots>)
    * `1`: [Aktuelles event](#aktuelles-event)

## Aktuelles event

In diesem Modus sendet der server jedes mal ein Paket, wenn sich der für den client sichtbare Zustand des aktuellen event ändert. Ein Paket hat folgende Varianten (durch das erste byte dargestellt):

* `0`: Ping
* `1`: Fehler
* `2`: Aktuell läuft kein event (wird nur zu Beginn der Verbindung oder wenn ein laufendes event endet geschickt)
* `3`: Aktuell läuft ein event (wird nur zu Beginn der Verbindung oder wenn ein event beginnt geschickt)
    * Gefolgt von der event ID als [`String`](https://docs.rs/async-proto/0.7/async_proto/trait.Protocol.html#impl-Protocol-for-String).
