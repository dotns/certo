package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/dotns/certo/pkg/api"
	"github.com/dotns/certo/pkg/certo"
	"github.com/dotns/certo/pkg/database"
	"github.com/dotns/certo/pkg/nameserver"

	"go.uber.org/zap"
)

var version = "dev"

func main() {
	setUmask()
	configPtr := flag.String("c", "./data/config.toml", "config file location")
	flag.Parse()
	// Read global config
	var err error
	var logger *zap.Logger
	config, err := certo.ReadConfig(*configPtr)
	if err != nil {
		fmt.Printf("Error: %s\n", err)
		os.Exit(1)
	}
	if port := os.Getenv("PORT"); port != "" {
		config.API.Port = port
	}
	logger, err = certo.SetupLogging(config)
	if err != nil {
		fmt.Printf("Could not set up logging: %s\n", err)
		os.Exit(1)
	}
	defer logger.Sync() //nolint:all
	sugar := logger.Sugar()

	sugar.Infow("Using config file",
		"file", *configPtr)
	if config.API.JWTSecret == "" {
		sugar.Warn("api.jwt_secret is empty: a random secret is generated per start, so dashboard sessions do not survive a restart and cannot be shared across replicas; set it for stable sessions")
	}
	sugar.Info("Starting up")
	db, err := database.Init(&config, sugar)
	if err != nil {
		// A failed init (bad connection, migration failure, unsupported db_version)
		// must stop startup — otherwise the servers run against an unusable/incompatible DB.
		sugar.Fatalw("Database initialization failed", "error", err)
	}
	// Error channel for servers
	errChan := make(chan error, 1)
	api := api.Init(&config, db, sugar, errChan, version)
	dnsservers := nameserver.InitAndStart(&config, db, sugar, errChan)
	go api.Start(dnsservers)
	for {
		err = <-errChan
		if err != nil {
			sugar.Fatal(err)
		}
	}
}
