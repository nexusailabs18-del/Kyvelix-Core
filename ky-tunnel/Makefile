CC = gcc
CFLAGS = -O3 -Wall -Wextra -std=gnu11
INCLUDES = -Iinclude
LDFLAGS = 
BUILD_DIR = build

CORE_SRCS = src/ky_protocol.c src/ky_frame.c src/ky_codec.c src/ky_tunnel.c
CORE_OBJS = $(patsubst src/%.c,$(BUILD_DIR)/%.o,$(CORE_SRCS))

TEST_SRCS = tests/test_protocol.c tests/test_frame.c tests/test_codec.c tests/test_tunnel.c
TEST_BINS = $(patsubst tests/%.c,$(BUILD_DIR)/%,$(TEST_SRCS))

all: core tests

# Create build directory
$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

# Compile core library objects
$(BUILD_DIR)/%.o: src/%.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(INCLUDES) -c $< -o $@

# Compile core library
core: $(CORE_OBJS)
	ar rcs $(BUILD_DIR)/libky_tunnel.a $(CORE_OBJS)
	@echo "✅ Core library built: $(BUILD_DIR)/libky_tunnel.a"

# Compile tests
$(BUILD_DIR)/test_protocol: tests/test_protocol.c $(BUILD_DIR)/ky_protocol.o | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(INCLUDES) $< $(BUILD_DIR)/ky_protocol.o -o $@

$(BUILD_DIR)/test_frame: tests/test_frame.c $(BUILD_DIR)/ky_frame.o $(BUILD_DIR)/ky_protocol.o | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(INCLUDES) $< $(BUILD_DIR)/ky_frame.o $(BUILD_DIR)/ky_protocol.o -o $@

$(BUILD_DIR)/test_codec: tests/test_codec.c $(BUILD_DIR)/ky_codec.o | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(INCLUDES) $< $(BUILD_DIR)/ky_codec.o -o $@

$(BUILD_DIR)/test_tunnel: tests/test_tunnel.c $(CORE_OBJS) | $(BUILD_DIR)
	$(CC) $(CFLAGS) $(INCLUDES) $< $(CORE_OBJS) -o $@

tests: $(TEST_BINS)
	@echo "✅ Tests built"

# Run all tests
test: tests
	@echo ""
	@echo "=== Running Tests ==="
	@echo ""
	@$(BUILD_DIR)/test_protocol
	@echo ""
	@$(BUILD_DIR)/test_frame
	@echo ""
	@$(BUILD_DIR)/test_codec
	@echo ""
	@$(BUILD_DIR)/test_tunnel
	@echo ""
	@echo "=== All Tests Complete ==="

# Clean build artifacts
clean:
	rm -rf $(BUILD_DIR)
	@echo "✅ Cleaned"

# Build Go bindings
go-build:
	cd go && go build ./...
	@echo "✅ Go bindings built"

# Install library
install: core
	cp $(BUILD_DIR)/libky_tunnel.a /usr/local/lib/
	cp include/*.h /usr/local/include/
	@echo "✅ Installed"

.PHONY: all core tests test clean go-build install
