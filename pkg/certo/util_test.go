package certo

import (
	"fmt"
	"math/rand/v2"
	"os"
	"testing"

	"go.uber.org/zap"
	"go.uber.org/zap/zapcore"
)

func fakeConfig() Config {
	conf := Config{}
	conf.Logconfig.Logtype = "stdout"
	return conf
}

func TestSetupLogging(t *testing.T) {
	conf := fakeConfig()
	for i, test := range []struct {
		format   string
		level    string
		expected zapcore.Level
	}{
		{"text", "warn", zap.WarnLevel},
		{"json", "debug", zap.DebugLevel},
		{"text", "info", zap.InfoLevel},
		{"json", "error", zap.ErrorLevel},
	} {
		conf.Logconfig.Format = test.format
		conf.Logconfig.Level = test.level
		logger, err := SetupLogging(conf)
		if err != nil {
			t.Errorf("Got unexpected error: %s", err)
		} else {
			if logger.Sugar().Level() != test.expected {
				t.Errorf("Test %d: Expected loglevel %s but got %s", i, test.expected, logger.Sugar().Level())
			}
		}
	}
}

func TestSetupLoggingError(t *testing.T) {
	conf := fakeConfig()
	for _, test := range []struct {
		format      string
		level       string
		file        string
		errexpected bool
	}{
		{"text", "warn", "", false},
		{"json", "debug", "", false},
		{"text", "info", "", false},
		{"json", "error", "", false},
		{"text", "something", "", true},
		{"text", "info", "a path with\" in its name.txt", false},
	} {
		conf.Logconfig.Format = test.format
		conf.Logconfig.Level = test.level
		if test.file != "" {
			conf.Logconfig.File = test.file
			conf.Logconfig.Logtype = "file"

		}
		_, err := SetupLogging(conf)
		if test.errexpected && err == nil {
			t.Errorf("Expected error but did not get one for loglevel: %s", err)
		} else if !test.errexpected && err != nil {
			t.Errorf("Unexpected error: %s", err)
		}

		// clean up the file zap creates
		if test.file != "" {
			_ = os.Remove(test.file)
		}
	}
}

func TestReadConfig(t *testing.T) {
	for i, test := range []struct {
		inFile []byte
		output Config
	}{
		{
			[]byte("[general]\nlisten = \":53\"\ndebug = true\n[api]\napi_domain = \"something.strange\""),
			Config{
				General: general{
					Listen: ":53",
					Debug:  true,
				},
				API: httpapi{
					Domain: "something.strange",
				},
			},
		},

		{
			[]byte("[\x00[[[[[[[[[de\nlisten =]"),
			Config{},
		},
	} {
		tmpfile, err := os.CreateTemp("", "acmedns")
		if err != nil {
			t.Fatalf("Could not create temporary file: %s", err)
		}
		defer os.Remove(tmpfile.Name())

		if _, err := tmpfile.Write(test.inFile); err != nil {
			t.Error("Could not write to temporary file")
		}

		if err := tmpfile.Close(); err != nil {
			t.Error("Could not close temporary file")
		}
		ret, _ := ReadConfig(tmpfile.Name())
		if ret.General.Listen != test.output.General.Listen {
			t.Errorf("Test %d: Expected listen value %s, but got %s", i, test.output.General.Listen, ret.General.Listen)
		}
		if ret.API.Domain != test.output.API.Domain {
			t.Errorf("Test %d: Expected HTTP API domain %s, but got %s", i, test.output.API.Domain, ret.API.Domain)
		}
	}
}

func TestReadConfigFromFile(t *testing.T) {
	testPath := "testdata/test_read_fallback_config.toml"

	cfg, err := ReadConfig(testPath)
	if err != nil {
		t.Fatalf("failed to read config file: %s", err)
	}
	if cfg.General.Listen != "127.0.0.1:53" {
		t.Errorf("Expected listen 127.0.0.1:53 but got %s", cfg.General.Listen)
	}
	if cfg.General.Domain != "test.example.org" {
		t.Errorf("Expected domain test.example.org but got %s", cfg.General.Domain)
	}
	if cfg.Database.Engine != "sqlite" {
		t.Errorf("Expected engine sqlite but got %s", cfg.Database.Engine)
	}
	if cfg.API.UseHeader != true {
		t.Errorf("Expected use_header true but got %t", cfg.API.UseHeader)
	}
}

func getNonExistentPath() (string, error) {
	path := fmt.Sprintf("/some/path/that/should/not/exist/on/any/filesystem/%10d.cfg", rand.Int())

	if _, err := os.Stat(path); os.IsNotExist(err) {
		return path, nil
	}

	return "", fmt.Errorf("attempted non existant file exists!?: %s", path)
}

func TestReadConfigNotFound(t *testing.T) {
	path, err := getNonExistentPath()
	if err != nil {
		t.Fatalf("failed getting non existent path: %s", err)
	}
	_, err = ReadConfig(path)
	if err == nil {
		t.Error("Should have failed reading non existent file")
	}
}

func TestReadConfigValidation(t *testing.T) {
	for i, test := range []struct {
		content     string
		shoulderror bool
	}{
		{`[database]
engine = "sqlite"
connection = "test.db"
[api]
tls = "none"`, false},
		{`[database]
engine = ""
connection = "test.db"`, false},
		{`[database]
engine = "sqlite"
connection = ""`, true},
		{`[database]
engine = "sqlite"
connection = "test.db"
[api]
tls = "invalid"`, true},
	} {
		tmpfile, err := os.CreateTemp("", "httpdns-cfg")
		if err != nil {
			t.Fatalf("Could not create temp file: %s", err)
		}
		_, _ = tmpfile.WriteString(test.content)
		tmpfile.Close()
		_, err = ReadConfig(tmpfile.Name())
		os.Remove(tmpfile.Name())
		if test.shoulderror && err == nil {
			t.Errorf("Test %d: Expected error but got none", i)
		}
		if !test.shoulderror && err != nil {
			t.Errorf("Test %d: Expected no error but got: %v", i, err)
		}
	}
}
