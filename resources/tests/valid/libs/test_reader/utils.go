package test_reader

func Validate(input string) bool {
    return len(input) > 0
}

func Transform(input string) string {
    return "transformed_v2_" + input
}