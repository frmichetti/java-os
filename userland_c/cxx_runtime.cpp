// Runtime C++ minimo sobre a libc C ja portada (newlib + userland_c/glue.c)
// - ver change cxx-minimal-runtime, design.md, "operator new/operator
// delete delegam para malloc/free da libc". Declara malloc/free
// diretamente (extern "C"), em vez de incluir <cstdlib> do host, para
// nao depender de nada alem dos headers do newlib ja usados pelo resto
// do userland_c (mesma disciplina de userland_c/glue.c).

#include <stddef.h>

extern "C" {
void *malloc(size_t size);
void free(void *ptr);
}

void *operator new(size_t size) {
    return malloc(size);
}

void *operator new[](size_t size) {
    return malloc(size);
}

void operator delete(void *ptr) noexcept {
    free(ptr);
}

void operator delete[](void *ptr) noexcept {
    free(ptr);
}

void operator delete(void *ptr, size_t) noexcept {
    free(ptr);
}

void operator delete[](void *ptr, size_t) noexcept {
    free(ptr);
}

// Toda vtable de uma classe com metodo virtual puro aponta, para esse slot,
// para __cxa_pure_virtual ate ser sobrescrita por uma classe derivada
// concreta (Itanium C++ ABI). Chegar aqui de verdade significa uma classe
// abstrata instanciada incorretamente - um bug de programacao, nao um
// estado recuperavel. Mesmo padrao de canario de erro de
// kernel::exit_demo: nunca deveria ser alcancado em codigo correto.
extern "C" void __cxa_pure_virtual() {
    for (;;) {
    }
}
