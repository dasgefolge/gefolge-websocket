Dieses Projekt ist ein [WebSocket](https://en.wikipedia.org/wiki/WebSocket) server für [die gefolge.org API](https://gefolge.org/api).

# Protokoll

Der server verwendet [async-proto 0.7](https://docs.rs/async-proto/0.7). Dementsprechend sind einzelne Pakete als binäre WebSocket-Nachrichten dargestellt.

## Verbindungsaufbau

1. Der client sendet seinen API key als [`String`](https://docs.rs/async-proto/0.7/async_proto/trait.Protocol.html#impl-Protocol-for-String).
2. Der client sendet ein byte, das den Zweck der Verbindung darstellt:
    * `0`: Rasende Roboter (siehe <https://github.com/dasgefolge/ricochet-robots>)
    * `1`: [Aktuelles event](#aktuelles-event)

## Aktuelles event

In diesem Modus sendet der server jedes mal ein Paket, wenn sich der für den client sichtbare Zustand des aktuellen event ändert. [Der Event-Beamer](https://github.com/dasgefolge/sil) verwendet diesen Modus. Ein Paket hat folgende Varianten (durch das erste byte dargestellt):

* `0`: Ping
* `1`: Fehler
* `2`: Aktuell läuft kein event mehr\*
* `3`: Aktuell läuft ein event\*
    * Gefolgt von der event ID als [`String`](https://docs.rs/async-proto/0.7/async_proto/trait.Protocol.html#impl-Protocol-for-String).
* `4`: Die aktuelle Version von [`sil`](https://github.com/dasgefolge/sil) hat sich geändert\*
    * Gefolgt vom aktuellen git commit hash als 20 bytes langer array.

\*Kann auch zu Beginn der Verbindung geschickt werden.
