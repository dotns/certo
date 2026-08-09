package certo

import (
	"errors"
	"fmt"

	"github.com/BurntSushi/toml"
)

const (
	TLSProviderNone               = "none"
	TLSProviderLetsEncrypt        = "letsencrypt"
	TLSProviderLetsEncryptStaging = "letsencryptstaging"
	TLSProviderCert               = "cert"
)

func ReadConfig(configFile string) (Config, error) {
	var conf Config
	_, err := toml.DecodeFile(configFile, &conf)
	if err != nil {
		return conf, fmt.Errorf("error reading configuration file %s: %w", configFile, err)
	}
	if conf.Database.Engine == "" {
		conf.Database.Engine = "sqlite"
	}
	if conf.Database.Engine != "sqlite" {
		return conf, fmt.Errorf("invalid database engine %q: only sqlite is supported", conf.Database.Engine)
	}
	if conf.Database.Connection == "" {
		return conf, errors.New("missing database configuration option \"connection\"")
	}
	if conf.API.ACMECacheDir == "" {
		conf.API.ACMECacheDir = "data/certs"
	}
	if conf.API.TLS == "" {
		conf.API.TLS = TLSProviderNone
	}
	switch conf.API.TLS {
	case TLSProviderCert, TLSProviderLetsEncrypt, TLSProviderLetsEncryptStaging, TLSProviderNone:
	default:
		return conf, fmt.Errorf("invalid value for api.tls: %s", conf.API.TLS)
	}
	return conf, nil
}
