// Command orbital-server is the dedicated Orbital game server.
// Bootstrap stub: binds the TCP lobby port and the UDP state port, then waits for a signal.
package main

import (
	"log"
	"net"
	"os"
	"os/signal"
	"syscall"
)

const (
	tcpAddr = ":4004" // lobby, login, reliable messages
	udpAddr = ":4005" // real-time game state
)

func main() {
	tcpLn, err := net.Listen("tcp", tcpAddr)
	if err != nil {
		log.Fatalf("listen tcp %s: %v", tcpAddr, err)
	}
	defer tcpLn.Close()

	udpConn, err := net.ListenPacket("udp", udpAddr)
	if err != nil {
		log.Fatalf("listen udp %s: %v", udpAddr, err)
	}
	defer udpConn.Close()

	log.Printf("orbital-server listening: tcp %s, udp %s", tcpAddr, udpAddr)

	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	<-sig
	log.Println("shutting down")
}
