CC ?= cc

CFLAGS += -std=c11 -Wall -Wextra -Iinclude -I..

SRC = \
	src/kyjit_bytecode.c \
	src/kyjit_validator.c

OBJ = $(SRC:.c=.o)

all: kyjit_check

kyjit_check: $(OBJ)
	$(CC) $(CFLAGS) -o $@ $(OBJ)

src/%.o: src/%.c
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f $(OBJ) kyjit_check