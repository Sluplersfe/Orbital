proto:
	PATH="$$PATH:$$(go env GOPATH)/bin" protoc -I ../proto --go_out=. --go_opt=module=orbital/server ../proto/orbital.proto
.PHONY: proto
