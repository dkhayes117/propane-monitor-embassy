/* Linker script for the nRF9160 in Non-secure mode */
MEMORY
{
    /* NOTE 1 K = 1 KiBi = 1024 bytes */
    FLASH                    : ORIGIN = 0x00050000, LENGTH = 704K
    RAM                      : ORIGIN = 0x20018000, LENGTH = 160K
}


