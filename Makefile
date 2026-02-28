# global make settings
.DEFAULT_GOAL := help
SHELL=/usr/bin/env bash

.SILENT:

# help settings
INFO=";набор команд для удобства при разработке;feel free to add some =) ;\
 для включения autocomplete по таргетам makefile в zsh нужно добавить в конец ~/.zshrc ;\
 zstyle ':completion:*:make:*:targets' call-command true ;\
 zstyle ':completion:*:make:*' tag-order targets ;\
 autoload -U compinit && compinit"

sep := ;
quotes := "
empty:=
space := $(empty)\n   $(empty)
INFO := $(subst $(sep),$(space),$(INFO))
INFO := $(subst $(quotes),$(empty),$(INFO))

### system
help: COL_WIDTH=30
help:  ## show this help
	@printf '\nUsage: make [task] ${INFO}\n\n'
	@printf " %-${COL_WIDTH}s %s\n" "task" "description"
	@printf " %-${COL_WIDTH}s %s\n" "----" "-----------"
	@grep -hE '^[a-zA-Z0-9_ \:\.-]+:.*?## .*$$|^###|^\s+##' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ": .*?## "}; \
		    {sub(/\\/, "", $$1)}; \
		    {if ($$1 ~ /^[[:space:]]+##/) { \
		        sub(/^[[:space:]]+##[[:space:]]*/, "", $$1); \
		        printf "\033[36m%-${COL_WIDTH}s\033[0m   %s\n", "", $$1; \
		    } else \
		    if ($$2 == "") printf " %s\n", $$1; \
    		else { \
    		    printf " \033[36m%s ", $$1; \
    		    for (i=length($$1); i<${COL_WIDTH}-1; i++) printf "."; \
    		    printf "\033[0m %s\n", $$2; \
	    	} \
    	}'
.PHONY: help

vars:  ## show configurable vars of Makefile
	@printf 'Configurable variables:\n'
	@awk '/^#:/{ gsub(/^#:[ \t]+/,""); printf "\033[36m%s\033[0m\n  ∟ ", $$0; getline; print }' $(MAKEFILE_LIST)
.PHONY: vars

###
BINARY := target/debug/b4n
LOG_DIR := logs

PORT1 := 7001
PORT2 := 7002
PORT3 := 7003
ADMIN_PORT1 := 17001
ADMIN_PORT2 := 17002
ADMIN_PORT3 := 17003


build:
	cargo build
.PHONY: build

start: build
	mkdir -p $(LOG_DIR)

	echo "Generating seeds..."
	OUT1="$$($(BINARY) --gen-seed)"; \
	SEED1="$$(printf '%s\n' "$$OUT1" | awk -F= '/^seed=/{print $$2}')"; \
	PUB1="$$(printf '%s\n' "$$OUT1" | awk -F= '/^pubkey=/{print $$2}')"; \
	ID1="$$(printf '%s\n' "$$OUT1" | awk -F= '/^node_id=/{print $$2}')"; \
    printf '%s\n' "$$ID1" > $(LOG_DIR)/id1; \
	printf '%s\n' "$$SEED1" > $(LOG_DIR)/seed1; \
	printf '%s\n' "$$PUB1"  > $(LOG_DIR)/pub1; \
	OUT2="$$($(BINARY) --gen-seed)"; \
	SEED2="$$(printf '%s\n' "$$OUT2" | awk -F= '/^seed=/{print $$2}')"; \
	PUB2="$$(printf '%s\n' "$$OUT2" | awk -F= '/^pubkey=/{print $$2}')"; \
	ID2="$$(printf '%s\n' "$$OUT2" | awk -F= '/^node_id=/{print $$2}')"; \
    printf '%s\n' "$$ID2" > $(LOG_DIR)/id2; \
	printf '%s\n' "$$SEED2" > $(LOG_DIR)/seed2; \
	printf '%s\n' "$$PUB2"  > $(LOG_DIR)/pub2; \
	OUT3="$$($(BINARY) --gen-seed)"; \
	SEED3="$$(printf '%s\n' "$$OUT3" | awk -F= '/^seed=/{print $$2}')"; \
	PUB3="$$(printf '%s\n' "$$OUT3" | awk -F= '/^pubkey=/{print $$2}')"; \
	ID3="$$(printf '%s\n' "$$OUT3" | awk -F= '/^node_id=/{print $$2}')"; \
    printf '%s\n' "$$ID3" > $(LOG_DIR)/id3; \
	printf '%s\n' "$$SEED3" > $(LOG_DIR)/seed3; \
	printf '%s\n' "$$PUB3"  > $(LOG_DIR)/pub3; \
	echo "Seeds generated."

	echo "Starting Node1..."
	$(BINARY) \
		--listen 127.0.0.1:$(PORT1) \
		--admin 127.0.0.1:${ADMIN_PORT1} \
		--seed $$(cat $(LOG_DIR)/seed1) \
		--validator-pubkey $$(cat $(LOG_DIR)/pub1) \
		--validator-pubkey $$(cat $(LOG_DIR)/pub2) \
		--validator-pubkey $$(cat $(LOG_DIR)/pub3) \
		--peer $$(cat $(LOG_DIR)/id2)@127.0.0.1:$(PORT2) \
		--peer $$(cat $(LOG_DIR)/id3)@127.0.0.1:$(PORT3) \
		> $(LOG_DIR)/node1.log 2>&1 & echo $$! > $(LOG_DIR)/node1.pid

	sleep 0.5

	echo "Starting Node2..."
	$(BINARY) \
		--listen 127.0.0.1:$(PORT2) \
		--admin 127.0.0.1:${ADMIN_PORT2} \
		--seed $$(cat $(LOG_DIR)/seed2) \
		--validator-pubkey $$(cat $(LOG_DIR)/pub1) \
		--validator-pubkey $$(cat $(LOG_DIR)/pub2) \
		--validator-pubkey $$(cat $(LOG_DIR)/pub3) \
		--peer $$(cat $(LOG_DIR)/id1)@127.0.0.1:$(PORT1) \
		--peer $$(cat $(LOG_DIR)/id3)@127.0.0.1:$(PORT3) \
		> $(LOG_DIR)/node2.log 2>&1 & echo $$! > $(LOG_DIR)/node2.pid

	sleep 0.5

	echo "Starting Node3..."
	$(BINARY) \
		--listen 127.0.0.1:$(PORT3) \
		--admin 127.0.0.1:${ADMIN_PORT3} \
		--seed $$(cat $(LOG_DIR)/seed3) \
		--validator-pubkey $$(cat $(LOG_DIR)/pub1) \
		--validator-pubkey $$(cat $(LOG_DIR)/pub2) \
		--validator-pubkey $$(cat $(LOG_DIR)/pub3) \
		--peer $$(cat $(LOG_DIR)/id1)@127.0.0.1:$(PORT1) \
		--peer $$(cat $(LOG_DIR)/id2)@127.0.0.1:$(PORT2) \
		> $(LOG_DIR)/node3.log 2>&1 & echo $$! > $(LOG_DIR)/node3.pid

	@echo "All nodes started."
.PHONY: start

stop:
	echo "Stopping nodes..."
	kill $$(cat $(LOG_DIR)/node1.pid) 2>/dev/null || true
	kill $$(cat $(LOG_DIR)/node2.pid) 2>/dev/null || true
	kill $$(cat $(LOG_DIR)/node3.pid) 2>/dev/null || true
	echo "Stopped."
.PHONY: stop


clean: stop
	rm -rf $(LOG_DIR)
.PHONY: clean

status1:
	@echo "print" | nc 127.0.0.1 $(ADMIN_PORT1)

status2:
	@echo "print" | nc 127.0.0.1 $(ADMIN_PORT2)

status3:
	@echo "print" | nc 127.0.0.1 $(ADMIN_PORT3)

status:
	echo "===== NODE 1 ====="
	echo "print" | nc 127.0.0.1 $(ADMIN_PORT1)
	tail -3 $(LOG_DIR)/node1.log
	echo ""
	echo "===== NODE 2 ====="
	echo "print" | nc 127.0.0.1 $(ADMIN_PORT2)
	tail -3 $(LOG_DIR)/node2.log
	echo ""
	echo "===== NODE 3 ====="
	echo "print" | nc 127.0.0.1 $(ADMIN_PORT3)
	tail -3 $(LOG_DIR)/node3.log
.PHONY: status


trx: text=$(or "hello", ${text})
trx:
	@echo "trx ${text}" | nc 127.0.0.1 $(ADMIN_PORT1)
.PHONY: trx
