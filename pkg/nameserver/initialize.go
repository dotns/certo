package nameserver

import (
	"fmt"
	"strings"
	"sync"

	"github.com/miekg/dns"
	"go.uber.org/zap"

	"github.com/dotns/certo/pkg/certo"
)

// Records is a slice of ResourceRecords
type Records struct {
	Records []dns.RR
}

type Nameserver struct {
	Config            *certo.Config
	DB                certo.DB
	Logger            *zap.SugaredLogger
	Server            *dns.Server
	OwnDomain         string
	NotifyStartedFunc func()
	SOA               dns.RR
	mu                sync.RWMutex
	personalAuthKey   string
	Domains           map[string]Records
	errChan           chan error
}

func InitAndStart(config *certo.Config, db certo.DB, logger *zap.SugaredLogger, errChan chan error) []certo.NS {
	dnsservers := make([]certo.NS, 0)
	waitLock := sync.Mutex{}
	if strings.HasPrefix(config.General.Proto, "both") {

		// Handle the case where DNS server should be started for both udp and tcp
		udpProto := "udp"
		tcpProto := "tcp"
		if strings.HasSuffix(config.General.Proto, "4") {
			udpProto += "4"
			tcpProto += "4"
		} else if strings.HasSuffix(config.General.Proto, "6") {
			udpProto += "6"
			tcpProto += "6"
		}
		dnsServerUDP := NewDNSServer(config, db, logger, udpProto)
		dnsservers = append(dnsservers, dnsServerUDP)
		dnsServerUDP.ParseRecords()
		dnsServerTCP := NewDNSServer(config, db, logger, tcpProto)
		dnsservers = append(dnsservers, dnsServerTCP)
		dnsServerTCP.ParseRecords()
		// wait for the server to get started to proceed
		waitLock.Lock()
		dnsServerUDP.SetNotifyStartedFunc(waitLock.Unlock)
		go dnsServerUDP.Start(errChan)
		waitLock.Lock()
		dnsServerTCP.SetNotifyStartedFunc(waitLock.Unlock)
		go dnsServerTCP.Start(errChan)
		waitLock.Lock()
	} else {
		dnsServer := NewDNSServer(config, db, logger, config.General.Proto)
		dnsservers = append(dnsservers, dnsServer)
		dnsServer.ParseRecords()
		waitLock.Lock()
		dnsServer.SetNotifyStartedFunc(waitLock.Unlock)
		go dnsServer.Start(errChan)
		waitLock.Lock()
	}
	return dnsservers
}

// NewDNSServer parses the DNS records from config and returns a new DNSServer struct
func NewDNSServer(config *certo.Config, db certo.DB, logger *zap.SugaredLogger, proto string) certo.NS {
	//		dnsServerTCP := NewDNSServer(DB, Config.General.Listen, tcpProto, Config.General.Domain)
	server := Nameserver{Config: config, DB: db, Logger: logger}
	server.Server = &dns.Server{Addr: config.General.Listen, Net: proto}
	domain := config.General.Domain
	if !strings.HasSuffix(domain, ".") {
		domain = domain + "."
	}
	server.OwnDomain = strings.ToLower(domain)
	server.personalAuthKey = ""
	server.Domains = make(map[string]Records)
	return &server
}

func (n *Nameserver) Start(errorChannel chan error) {
	n.errChan = errorChannel
	dns.HandleFunc(".", n.handleRequest)
	n.Logger.Infow("Starting DNS listener",
		"addr", n.Server.Addr,
		"proto", n.Server.Net)
	if n.NotifyStartedFunc != nil {
		n.Server.NotifyStartedFunc = n.NotifyStartedFunc
	}
	err := n.Server.ListenAndServe()
	if err != nil {
		errorChannel <- fmt.Errorf("DNS server %s failed: %w", n.Server.Net, err)
	}
}

func (n *Nameserver) SetNotifyStartedFunc(fun func()) {
	n.Server.NotifyStartedFunc = fun
}
