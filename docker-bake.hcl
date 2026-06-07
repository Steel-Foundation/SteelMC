group "default" {
  targets = ["normal", "pelican"]
}

target "_common" {
  context    = "."
  dockerfile = "./docker/Dockerfile"
  platforms  = ["linux/amd64", "linux/arm64"]
}

target "normal" {
  inherits = ["_common"]
  target   = "normal"
}

target "pelican" {
  inherits = ["_common"]
  target   = "pelican"
}
