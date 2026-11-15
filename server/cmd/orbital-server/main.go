// Command orbital-server is the dedicated Orbital game server.
//
//	orbital-server [-tcp :4004] [-udp :4005] [-tokens tokens.txt]
//	orbital-server token NAME [-tokens tokens.txt]   # trust a player, print their join token
package main

import (
	"flag"
	"fmt"
	"log"
	"os"
	"os/signal"
	"syscall"

	"orbital/server/internal/server"
	"orbital/server/internal/session"
)

func main() {
	if len(os.Args) > 1 && os.Args[1] == "token" {
		issue(os.Args[2:])
		return
	}
	tcp := flag.String("tcp", ":4004", "lobby, login and reliable messages")
	udp := flag.String("udp", ":4005", "real-time game state")
	tokens := flag.String("tokens", "tokens.txt", `join tokens file, one "name token" per line`)
	flag.Parse()

	sess := session.New()
	n, err := sess.LoadTokens(*tokens)
	if err != nil {
		log.Printf("no tokens loaded (%v): nobody can join until you run `orbital-server token NAME`", err)
	}
	s, err := server.Listen(server.Config{TCPAddr: *tcp, UDPAddr: *udp, Session: sess})
	if err != nil {
		log.Fatal(err)
	}
	s.Serve()
	log.Printf("orbital-server listening: tcp %s, udp %s, %d trusted players", s.TCPAddr(), s.UDPAddr(), n)

	sig := make(chan os.Signal, 1)
	signal.Notify(sig, syscall.SIGINT, syscall.SIGTERM)
	<-sig
	log.Println("shutting down")
	s.Close()
}

func issue(args []string) {
	fs := flag.NewFlagSet("token", flag.ExitOnError)
	tokens := fs.String("tokens", "tokens.txt", "join tokens file")
	if len(args) == 0 {
		log.Fatal("usage: orbital-server token NAME [-tokens file]")
	}
	name := args[0]
	fs.Parse(args[1:])
	tok := session.NewToken()
	f, err := os.OpenFile(*tokens, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600)
	if err != nil {
		log.Fatal(err)
	}
	defer f.Close()
	if _, err := fmt.Fprintf(f, "%s %s\n", name, tok); err != nil {
		log.Fatal(err)
	}
	fmt.Println(tok)
}
