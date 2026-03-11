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
#: DOCKER_IMAGE - local docker image tag used for docker-based tasks
DOCKER_IMAGE ?= b4n
#: SECRETS_DIR - output directory for generated node secrets
SECRETS_DIR ?= secrets
#: VALIDATORS - number of validator nodes to generate in `make gen-secrets`
VALIDATORS ?= 3
#: ORDINARY - number of ordinary non-validator nodes to generate in `make gen-secrets`
ORDINARY ?= 0
#: VALIDATOR_HOSTS - comma-separated validator hosts for `make print-deploy-commands`
VALIDATOR_HOSTS ?=
#: VALIDATOR_NODE_PORTS - comma-separated blockchain ports for validators, defaults to HOST_NODE_PORT for every validator
VALIDATOR_NODE_PORTS ?=
#: VALIDATOR_WEB_PORTS - comma-separated web ports for validators, defaults to HOST_WEB_PORT for every validator
VALIDATOR_WEB_PORTS ?=
#: HOST_NODE_PORT - external p2p port used on every server
HOST_NODE_PORT ?= 7001
#: HOST_WEB_PORT - external web port used on every server
HOST_WEB_PORT ?= 8080
#: CONTAINER_PREFIX - prefix for container names in deploy commands
CONTAINER_PREFIX ?= b4

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
	#rm -rf $(LOG_DIR)
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

gen-secrets: ## generate validator/non-validator seeds and env files into $(SECRETS_DIR)
	##  make gen-secrets DOCKER_IMAGE=b4-web-test VALIDATORS=3 ORDINARY=20 SECRETS_DIR=./secrets
	image="$(DOCKER_IMAGE)"; \
	secrets_dir="$(SECRETS_DIR)"; \
	validators_total="$(VALIDATORS)"; \
	ordinary_total="$(ORDINARY)"; \
	if (( validators_total < 1 )); then \
		echo "VALIDATORS must be >= 1"; \
		exit 1; \
	fi; \
	if (( ordinary_total < 0 )); then \
		echo "ORDINARY must be >= 0"; \
		exit 1; \
	fi; \
	if ! docker image inspect "$$image" >/dev/null 2>&1; then \
		echo "Docker image '$$image' not found locally, building it first..."; \
		docker build -t "$$image" .; \
	fi; \
	mkdir -p "$$secrets_dir"; \
	chmod 700 "$$secrets_dir"; \
	rm -f "$$secrets_dir"/validator*.env "$$secrets_dir"/ordinary*.env "$$secrets_dir"/validators-public.env; \
	declare -a validator_pubkeys=(); \
	declare -a validator_ids=(); \
	gen_env() { \
		local name="$$1"; \
		local out seed pub node_id; \
		out="$$(docker run --rm --entrypoint /usr/local/bin/b4n "$$image" --gen-seed)"; \
		seed="$$(printf '%s\n' "$$out" | awk -F= '/^seed=/{print $$2}')"; \
		pub="$$(printf '%s\n' "$$out" | awk -F= '/^pubkey=/{print $$2}')"; \
		node_id="$$(printf '%s\n' "$$out" | awk -F= '/^node_id=/{print $$2}')"; \
		printf 'SEED=%s\nPUBKEY=%s\nNODE_ID=%s\n' "$$seed" "$$pub" "$$node_id" > "$$secrets_dir/$$name.env"; \
		chmod 600 "$$secrets_dir/$$name.env"; \
		printf 'generated %s -> %s\n' "$$name" "$$secrets_dir/$$name.env"; \
		GEN_PUBKEY="$$pub"; \
		GEN_NODE_ID="$$node_id"; \
	}; \
	for ((i = 1; i <= validators_total; i++)); do \
		gen_env "validator$$i"; \
		validator_pubkeys+=("$$GEN_PUBKEY"); \
		validator_ids+=("$$GEN_NODE_ID"); \
	done; \
	for ((i = 1; i <= ordinary_total; i++)); do \
		gen_env "ordinary$$i"; \
	done; \
	validator_csv="$$(IFS=,; printf '%s' "$${validator_pubkeys[*]}")"; \
	{ \
		printf 'B4_VALIDATOR_PUBKEYS=%s\n' "$$validator_csv"; \
		for ((i = 1; i <= validators_total; i++)); do \
			printf 'VALIDATOR_%s_ID=%s\n' "$$i" "$${validator_ids[$$((i - 1))]}"; \
		done; \
	} > "$$secrets_dir/validators-public.env"; \
	chmod 600 "$$secrets_dir/validators-public.env"; \
	printf 'generated %s/validators-public.env\n' "$$secrets_dir"
.PHONY: gen-secrets

print-deploy-commands: ## print ready-to-run docker commands with all values already substituted
	##  make print-deploy-commands SECRETS_DIR=./secrets DOCKER_IMAGE=registry.example.com/b4-web:latest VALIDATOR_HOSTS=v1.example.com,v2.example.com,v3.example.com VALIDATOR_NODE_PORTS=7001,7101,7201 VALIDATOR_WEB_PORTS=8080,8081,8082
	secrets_dir="$(SECRETS_DIR)"; \
	image="$(DOCKER_IMAGE)"; \
	validator_hosts_raw="$(VALIDATOR_HOSTS)"; \
	validator_node_ports_raw="$(VALIDATOR_NODE_PORTS)"; \
	validator_web_ports_raw="$(VALIDATOR_WEB_PORTS)"; \
	host_node_port="$(HOST_NODE_PORT)"; \
	host_web_port="$(HOST_WEB_PORT)"; \
	container_prefix="$(CONTAINER_PREFIX)"; \
	public_env="$$secrets_dir/validators-public.env"; \
	shopt -s nullglob; \
	validator_files=("$$secrets_dir"/validator[0-9]*.env); \
	ordinary_files=("$$secrets_dir"/ordinary[0-9]*.env); \
	if [[ ! -f "$$public_env" ]]; then \
		echo "Missing $$public_env. Run 'make gen-secrets' first."; \
		exit 1; \
	fi; \
	if (( $${#validator_files[@]} == 0 )); then \
		echo "No validator*.env files found in $$secrets_dir"; \
		exit 1; \
	fi; \
	declare -a validator_hosts=(); \
	if [[ -n "$$validator_hosts_raw" ]]; then \
		IFS=',' read -r -a validator_hosts <<< "$$validator_hosts_raw"; \
		if (( $${#validator_hosts[@]} != $${#validator_files[@]} )); then \
			echo "VALIDATOR_HOSTS count must match validator env files count ($${#validator_files[@]})"; \
			exit 1; \
		fi; \
	else \
		for ((i = 1; i <= $${#validator_files[@]}; i++)); do \
			validator_hosts+=("validator$$i.example.com"); \
		done; \
	fi; \
	declare -a validator_node_ports=(); \
	if [[ -n "$$validator_node_ports_raw" ]]; then \
		IFS=',' read -r -a validator_node_ports <<< "$$validator_node_ports_raw"; \
		if (( $${#validator_node_ports[@]} != $${#validator_files[@]} )); then \
			echo "VALIDATOR_NODE_PORTS count must match validator env files count ($${#validator_files[@]})"; \
			exit 1; \
		fi; \
	else \
		for ((i = 1; i <= $${#validator_files[@]}; i++)); do \
			validator_node_ports+=("$$host_node_port"); \
		done; \
	fi; \
	declare -a validator_web_ports=(); \
	if [[ -n "$$validator_web_ports_raw" ]]; then \
		IFS=',' read -r -a validator_web_ports <<< "$$validator_web_ports_raw"; \
		if (( $${#validator_web_ports[@]} != $${#validator_files[@]} )); then \
			echo "VALIDATOR_WEB_PORTS count must match validator env files count ($${#validator_files[@]})"; \
			exit 1; \
		fi; \
	else \
		for ((i = 1; i <= $${#validator_files[@]}; i++)); do \
			validator_web_ports+=("$$host_web_port"); \
		done; \
	fi; \
	source "$$public_env"; \
	declare -a validator_ids=(); \
	for ((i = 1; i <= $${#validator_files[@]}; i++)); do \
		eval "validator_ids+=(\"\$${VALIDATOR_$${i}_ID}\")"; \
	done; \
	for ((i = 1; i <= $${#validator_files[@]}; i++)); do \
		host="$${validator_hosts[$$((i - 1))]}"; \
		node_port="$${validator_node_ports[$$((i - 1))]}"; \
		web_port="$${validator_web_ports[$$((i - 1))]}"; \
		container_name="$$container_prefix-validator$$i"; \
		source "$$secrets_dir/validator$$i.env"; \
		node_seed="$$SEED"; \
		peer_entries=(); \
		for ((j = 1; j <= $${#validator_files[@]}; j++)); do \
			if (( j == i )); then \
				continue; \
			fi; \
			peer_entries+=("$${validator_ids[$$((j - 1))]}@$${validator_hosts[$$((j - 1))]}:$${validator_node_ports[$$((j - 1))]}"); \
		done; \
		validator_peers="$$(IFS=,; printf '%s' "$${peer_entries[*]}")"; \
		printf 'docker pull %s\n' "$$image"; \
		printf 'docker rm -f %s >/dev/null 2>&1 || true\n' "$$container_name"; \
		printf 'docker run -d --restart unless-stopped --name %s -p %s:%s -p %s:%s -e WEB_PORT=%s -e B4_LISTEN=0.0.0.0:%s -e B4_ADMIN=0.0.0.0:17001 -e B4_SEED=%s -e B4_VALIDATOR_PUBKEYS=%s -e B4_PEERS=%s %s\n' "$$container_name" "$$node_port" "$$node_port" "$$web_port" "$$web_port" "$$web_port" "$$node_port" "$$node_seed" "$$B4_VALIDATOR_PUBKEYS" "$$validator_peers" "$$image"; \
		printf '\n'; \
	done; \
	for ((i = 1; i <= $${#ordinary_files[@]}; i++)); do \
		container_name="$$container_prefix-ordinary$$i"; \
		source "$$secrets_dir/ordinary$$i.env"; \
		node_seed="$$SEED"; \
		peer_entries=(); \
		for ((j = 1; j <= $${#validator_files[@]}; j++)); do \
			peer_entries+=("$${validator_ids[$$((j - 1))]}@$${validator_hosts[$$((j - 1))]}:$${validator_node_ports[$$((j - 1))]}"); \
		done; \
		ordinary_peers="$$(IFS=,; printf '%s' "$${peer_entries[*]}")"; \
		printf 'docker pull %s\n' "$$image"; \
		printf 'docker run --rm -d --name %s -p %s:%s -p %s:%s -e WEB_PORT=%s -e B4_LISTEN=0.0.0.0:%s -e B4_ADMIN=0.0.0.0:17001 -e B4_SEED=%s -e B4_VALIDATOR_PUBKEYS=%s -e B4_PEERS=%s %s\n' "$$container_name" "$$host_node_port" "$$host_node_port" "$$host_web_port" "$$host_web_port" "$$host_web_port" "$$host_node_port" "$$node_seed" "$$B4_VALIDATOR_PUBKEYS" "$$ordinary_peers" "$$image"; \
		printf '\n'; \
	done
.PHONY: print-deploy-commands


dbuild:
	docker buildx build --platform linux/amd64 . -t b4n
.PHONY: dbuild
