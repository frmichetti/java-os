# JAVA OS — Projeto de Sistema Operacional Próprio

Atue como arquiteto de sistemas operacionais e engenheiro especialista em Rust, Assembly x86_64, JVM, OpenJDK e desenvolvimento de emuladores.

Quero desenvolver um sistema operacional inteiramente próprio, sem utilizar Linux ou Windows como kernel.

## Objetivo

Criar o Java OS, um sistema operacional de 64 bits cuja principal responsabilidade será executar aplicações Java.

## Arquitetura obrigatória

**1. Kernel**
- Desenvolvido em Rust (`no_std`) e Assembly.
- Arquitetura x86_64.
- Boot UEFI.
- Bootloader Limine.
- Gerenciamento de memória.
- Processos e threads.
- Escalonador.
- Interrupções.
- System calls.
- Gerenciamento de I/O.
- Drivers e abstração de hardware.
- Isolamento de memória e processos.

**2. Java Runtime**
- Avaliar o porte do OpenJDK/HotSpot.
- Implementar integração entre JVM e kernel.
- Suportar Garbage Collector e JIT.
- Suportar aplicações JAR.
- Permitir diferentes versões do Java.
- Compatibilidade com aplicações Java tradicionais.

**3. Java OS Platform**

Desenvolvida prioritariamente em Java.

Responsabilidades:
- Gerenciamento de aplicações.
- Gerenciamento de arquivos.
- Serviços do sistema.
- Interface gráfica.
- Gerenciamento de dispositivos.
- Rede e comunicação.
- Gerenciamento de processos Java.
- Atualizações e configurações.

**4. Gaming Platform**

Desenvolvida em Java.

- Game Launcher.
- ROM Manager.
- Emulator Manager.
- Save Manager.
- Controller Manager.
- Emuladores próprios.
- Biblioteca de jogos.
- Gerenciamento de controles.

## Modelo de execução

Avaliar um modelo híbrido de JVM:

- Runtime compartilhado para serviços controlados do sistema.
- JVM isolada para aplicações de terceiros.
- Controle de memória e permissões por aplicação.
- Compatibilidade com aplicações Java tradicionais e aplicações próprias do Java OS.

## Hardware suportado

- Desktop x86_64.
- Notebook x86_64.
- QEMU.
- VMware.
- VirtualBox.

Não implementar suporte a 32 bits ou ARM64 nesta etapa.

## Tecnologias iniciais

| Componente | Tecnologia |
|---|---|
| Kernel | Rust |
| Assembly | x86_64 |
| Bootloader | Limine |
| Firmware | UEFI / OVMF |
| Runtime | OpenJDK |
| Desenvolvimento | Cargo |
| Virtualização | QEMU |
| Depuração | GDB |
| Versionamento | Git |
| CI/CD | GitHub Actions |

## Processo de desenvolvimento

Não tente desenvolver o sistema inteiro de uma vez.

Trabalhe com especificações técnicas, arquitetura modular, documentação e entregas incrementais.

Cada etapa deverá possuir:
- Objetivos.
- Arquivos envolvidos.
- Implementação.
- Testes automatizados.
- Critérios de aceitação.
- Documentação.

Não apresente pseudocódigo como implementação funcional.

## Primeiro entregável

Crie a arquitetura técnica completa do Java OS, contendo:

1. Arquitetura do kernel.
2. Arquitetura de memória.
3. Gerenciamento de processos e threads.
4. System calls.
5. HAL e drivers.
6. Estratégia de portabilidade do OpenJDK.
7. Arquitetura da JVM.
8. Modelo de execução das aplicações.
9. APIs Java próprias.
10. Arquitetura gráfica.
11. Arquitetura de emulação.
12. Estrutura de repositórios.
13. Pipeline de compilação.
14. Estratégia de testes.
15. Roadmap completo.

O primeiro objetivo executável deverá ser inicializar nosso próprio kernel Rust x86_64 no QEMU através de UEFI, apresentar informações de inicialização e memória e estabelecer uma base de testes automatizados.

Antes de escrever código, apresente a arquitetura e identifique os riscos técnicos, especialmente a complexidade de portar uma JVM completa para um sistema operacional próprio.

O resultado esperado é um sistema operacional independente, com kernel próprio e Java como principal plataforma de execução.