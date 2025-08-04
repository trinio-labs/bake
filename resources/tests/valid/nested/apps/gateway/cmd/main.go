package main

import (
    "fmt"
    "test_reader" // This would import from ../../../../libs/test_reader
)

func main() {
    config := test_reader.ReadConfig()
    fmt.Println("Config:", config)
}