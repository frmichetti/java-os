// Programa C++ de verificacao do runtime C++ minimo (change
// cxx-minimal-runtime) - ver design.md/tasks.md, grupo 3. Carregado via o
// carregador ELF ja existente (kernel::elf/spawn_from_elf), mesmo padrao de
// userland_c/libc_test_program.c.
//
// Exercita: 1) despacho de metodo virtual atraves de um ponteiro de
// classe-base apontando para uma instancia alocada com `new` de uma classe
// derivada (confirma que a implementacao derivada, nao a da base, e
// executada - e que o campo escrito no objeto alocado e visivel, provando
// que a memoria e utilizavel); 2) `delete` atraves do mesmo ponteiro de
// classe-base (exercita o destrutor virtual) sem travar.

#include <unistd.h>

class Shape {
public:
    virtual ~Shape() {}
    virtual int area() const = 0;
    int tag = 0;
};

class Square : public Shape {
public:
    explicit Square(int side) : side_(side) {}
    int area() const override { return side_ * side_; }

private:
    int side_;
};

extern "C" int main() {
    char result[2] = {'?', '?'};

    result[0] = 'F';
    Shape *s = new Square(7);
    if (s != nullptr) {
        s->tag = 0x5A; // padrao conhecido, escrito no objeto alocado por `new`
        if (s->area() == 49 && s->tag == 0x5A) {
            // area() == 49 so acontece se o despacho virtual chamou
            // Square::area(), nao uma implementacao de Shape (que nao
            // existe, ja que area() e puro - chegar aqui sem travar em
            // __cxa_pure_virtual ja e parte da prova).
            result[0] = 'P';
        }
    }

    delete s; // exercita o destrutor virtual via ponteiro de classe-base
    result[1] = 'P'; // chegar aqui sem travar confirma que delete nao corrompeu nada

    const char prefix[] = "cxx_test_program: resultado ";
    const char suffix[] = " (ordem: virtual_dispatch_new, delete)\n";
    char msg[128];
    unsigned long len = 0;
    for (unsigned long i = 0; prefix[i] != '\0'; i++) {
        msg[len++] = prefix[i];
    }
    msg[len++] = result[0];
    msg[len++] = result[1];
    for (unsigned long i = 0; suffix[i] != '\0'; i++) {
        msg[len++] = suffix[i];
    }
    write(1, msg, len);

    return 0;
}
