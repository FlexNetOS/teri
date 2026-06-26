.PHONY: help setup dev deps clean build test lint format check-security db-up db-down db-reset migrate check-docker install-docker

# OS Detection
UNAME_S := $(shell uname -s)
UNAME_M := $(shell uname -m)
OS_ID := $(shell grep -oP '(?<=^ID=).+' /etc/os-release 2>/dev/null || echo 'unknown')
OS_LIKE := $(shell grep -oP '(?<=^ID_LIKE=).+' /etc/os-release 2>/dev/null || echo 'unknown')

# Default target
help:
	@echo "Pebesen Development Commands"
	@echo ""
	@echo "Setup & Dependencies:"
	@echo "  setup          - Install all dependencies and set up environment"
	@echo "  deps           - Install/update all project dependencies"
	@echo "  check-security - Run security audits on dependencies"
	@echo "  check-docker   - Check if Docker is installed"
	@echo "  install-docker - Install Docker (auto-detects OS)"
	@echo ""
	@echo "Development:"
	@echo "  dev            - Start full development environment"
	@echo "  dev-backend    - Start only backend services"
	@echo "  dev-frontend   - Start only frontend development server"
	@echo "  build          - Build all project components"
	@echo ""
	@echo "Database:"
	@echo "  db-up          - Start database services"
	@echo "  db-down        - Stop database services"
	@echo "  db-reset       - Reset database (WARNING: destroys data)"
	@echo "  migrate        - Run database migrations"
	@echo ""
	@echo "Quality Assurance:"
	@echo "  test           - Run all tests"
	@echo "  lint           - Run all linters"
	@echo "  format         - Format all code"
	@echo "  check          - Run all checks (format + lint + test)"
	@echo ""
	@echo "Utilities:"
	@echo "  clean          - Clean build artifacts and caches"
	@echo "  reindex        - Rebuild search index"
	@echo "  logs           - Show development logs"

# Docker Management
check-docker:
	@echo "Checking Docker installation..."
	@which docker > /dev/null 2>&1 && { echo "✓ Docker is installed"; docker --version; } || { echo "✗ Docker is not installed. Run 'make install-docker' to install."; exit 1; }
	@docker compose version > /dev/null 2>&1 && echo "✓ Docker Compose is available" || { echo "✗ Docker Compose is not available"; exit 1; }

install-docker:
	@echo "Installing Docker for OS: $(OS_ID) (like: $(OS_LIKE))..."
ifeq ($(UNAME_S),Linux)
ifeq ($(OS_ID),ubuntu)
	@echo "Detected Ubuntu. Installing Docker..."
	@sudo apt-get update
	@sudo apt-get install -y ca-certificates curl gnupg
	@sudo install -m 0755 -d /etc/apt/keyrings
	@curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
	@sudo chmod a+r /etc/apt/keyrings/docker.gpg
	@echo "deb [arch=$(shell dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu $(shell . /etc/os-release && echo $$VERSION_CODENAME) stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
	@sudo apt-get update
	@sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
	@echo "Docker installed successfully!"
	@echo "Adding current user to docker group..."
	@sudo usermod -aG docker $(USER)
	@echo "Please log out and log back in for group changes to take effect."
else ifeq ($(OS_ID),debian)
	@echo "Detected Debian. Installing Docker..."
	@sudo apt-get update
	@sudo apt-get install -y ca-certificates curl gnupg
	@sudo install -m 0755 -d /etc/apt/keyrings
	@curl -fsSL https://download.docker.com/linux/debian/gpg | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
	@sudo chmod a+r /etc/apt/keyrings/docker.gpg
	@echo "deb [arch=$(shell dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] https://download.docker.com/linux/debian $(shell . /etc/os-release && echo $$VERSION_CODENAME) stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
	@sudo apt-get update
	@sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
	@echo "Docker installed successfully!"
	@sudo usermod -aG docker $(USER)
	@echo "Please log out and log back in for group changes to take effect."
else ifeq ($(OS_ID),fedora)
	@echo "Detected Fedora. Installing Docker..."
	@sudo dnf -y install dnf-plugins-core
	@sudo dnf config-manager --add-repo https://download.docker.com/linux/fedora/docker-ce.repo
	@sudo dnf install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
	@sudo systemctl start docker
	@sudo systemctl enable docker
	@echo "Docker installed successfully!"
	@sudo usermod -aG docker $(USER)
	@echo "Please log out and log back in for group changes to take effect."
else ifeq ($(OS_ID),arch)
	@echo "Detected Arch Linux. Installing Docker..."
	@sudo pacman -Syu --noconfirm docker docker-compose
	@sudo systemctl start docker
	@sudo systemctl enable docker
	@echo "Docker installed successfully!"
	@sudo usermod -aG docker $(USER)
	@echo "Please log out and log back in for group changes to take effect."
else ifneq (,$(findstring rhel,$(OS_LIKE)))
	@echo "Detected RHEL-based system. Installing Docker..."
	@sudo dnf install -y yum-utils
	@sudo yum-config-manager --add-repo https://download.docker.com/linux/centos/docker-ce.repo
	@sudo dnf install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
	@sudo systemctl start docker
	@sudo systemctl enable docker
	@echo "Docker installed successfully!"
	@sudo usermod -aG docker $(USER)
	@echo "Please log out and log back in for group changes to take effect."
else
	@echo "Unsupported Linux distribution: $(OS_ID)"
	@echo "Please install Docker manually: https://docs.docker.com/engine/install/"
	@exit 1
endif
else ifeq ($(UNAME_S),Darwin)
	@echo "Detected macOS."
	@which brew > /dev/null 2>&1 || { echo "Homebrew is required. Install from https://brew.sh"; exit 1; }
	@echo "Installing Docker via Homebrew..."
	@brew install --cask docker
	@echo "Docker installed! Please start Docker Desktop from Applications."
else
	@echo "Unsupported operating system: $(UNAME_S)"
	@echo "Please install Docker manually: https://docs.docker.com/engine/install/"
	@exit 1
endif

# Setup & Dependencies
setup:
	@echo "Setting up Pebesen development environment..."
	@echo "Installing Rust toolchain..."
	rustup update stable
	rustup component add rustfmt clippy
	@echo "Installing sqlx-cli..."
	cargo install sqlx-cli --no-default-features --features native-tls,postgres || true
	@echo "Installing cargo-audit..."
	cargo install cargo-audit || true
	@echo "Installing pre-commit..."
	uv tool install pre-commit || pip install pre-commit || true
	@echo "Installing pre-commit hooks..."
	uv run pre-commit install || pre-commit install || true
	@echo "Installing frontend dependencies..."
	cd frontend && pnpm install
	@echo "Starting database services..."
	$(MAKE) db-up
	@echo "Running migrations..."
	$(MAKE) migrate
	@echo "Setup complete! Run 'make dev' to start development."

deps:
	@echo "Updating all dependencies..."
	@echo "Updating Rust dependencies..."
	cargo update
	@echo "Updating frontend dependencies..."
	cd frontend && pnpm update
	@echo "Dependencies updated!"

check-security:
	@echo "Running security audits..."
	@echo "Auditing Rust dependencies..."
	cargo audit
	@echo "Auditing frontend dependencies..."
	cd frontend && pnpm audit --audit-level moderate

# Development
dev:
	@echo "Starting full development environment..."
	$(MAKE) db-up
	@echo "Starting Rust development server..."
	cargo watch -x run &
	@echo "Starting frontend development server..."
	cd frontend && pnpm dev &
	@echo "Development environment started!"
	@echo "Frontend: http://localhost:5173"
	@echo "Backend: http://localhost:3000 (when implemented)"

dev-backend:
	@echo "Starting backend development environment..."
	$(MAKE) db-up
	cargo watch -x run

dev-frontend:
	@echo "Starting frontend development server..."
	cd frontend && pnpm dev

build:
	@echo "Building all components..."
	@echo "Building Rust workspace..."
	cargo build --release
	@echo "Building frontend..."
	cd frontend && pnpm build
	@echo "Build complete!"

# Database Management
db-up: check-docker
	@echo "Starting database services..."
	docker compose up -d postgres redis meilisearch
	@echo "Waiting for services to be ready..."
	sleep 10
	@echo "Database services started!"

db-down:
	@echo "Stopping database services..."
	docker compose down postgres redis meilisearch
	@echo "Database services stopped!"

db-reset:
	@echo "Resetting database (WARNING: This will destroy all data)..."
	docker compose down -v
	docker compose up -d postgres redis meilisearch
	sleep 10
	$(MAKE) migrate
	@echo "Database reset complete!"

migrate:
	@echo "Running database migrations..."
	docker exec -i pebesen_postgres psql -U pebesen -d pebesen < migrations/0001_extensions.sql
	@echo "Migrations complete!"

# Quality Assurance
test:
	@echo "Running all tests..."
	@echo "Running Rust tests..."
	cargo test --all-features
	@echo "Running frontend tests..."
	cd frontend && pnpm test
	@echo "All tests complete!"

lint:
	@echo "Running all linters..."
	@echo "Running Rust linters..."
	cargo fmt --check
	cargo clippy --all-targets --all-features -- -D warnings
	@echo "Running frontend linters..."
	cd frontend && pnpm lint
	@echo "Linting complete!"

format:
	@echo "Formatting all code..."
	@echo "Formatting Rust code..."
	cargo fmt
	@echo "Formatting frontend code..."
	cd frontend && pnpm format
	@echo "Code formatting complete!"

check: format lint test
	@echo "All checks completed successfully!"

# Utilities
clean:
	@echo "Cleaning build artifacts and caches..."
	@echo "Cleaning Rust artifacts..."
	cargo clean
	@echo "Cleaning frontend artifacts..."
	cd frontend && rm -rf .svelte-kit build dist node_modules/.cache
	@echo "Cleaning Docker resources..."
	docker compose down -v || true
	docker system prune -f || true
	@echo "Cleanup complete!"

reindex:
	@echo "Rebuilding search index..."
	cargo run --bin reindex -- --space all

logs:
	@echo "Showing development logs..."
	docker compose logs -f
