package main

import (
	"encoding/json"
	"os"

	oresmiddleware "github.com/ORESoftware/ores-middleware/src/golang"
)

func main() {
	encoder := json.NewEncoder(os.Stdout)
	if err := encoder.Encode(oresmiddleware.Descriptor()); err != nil { panic(err) }
}
