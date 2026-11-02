# Known Issues

## ~~IRQ1 (teclado) entregue só uma vez por sessão de boot a 100 Hz sob QEMU/TCG~~ (NÃO É UM BUG DO KERNEL)

**Status:** investigado a fundo na change `kernel-keyboard` (ver `openspec/changes/archive/.../design.md`, seção "Achados durante a implementação") — confirmado como limitação de timing da emulação por software do QEMU (TCG), não um defeito do kernel. Nenhuma correção de código necessária.

**Sintoma observado:** com o escalonador na frequência padrão de 100 Hz, uma tecla simulada via `sendkey`/QMP no QEMU era recebida e traduzida corretamente, mas nenhuma tecla seguinte (mesmo minutos depois, em conexões separadas) disparava uma nova interrupção. `info lapic` do monitor do QEMU mostrava o vetor do teclado permanentemente pendente em IRR, nunca promovido a ISR.

**Diagnóstico:** descartados como causa (todos testados e refutados com evidência real via GDB/`info lapic`): entrada do I/O APIC mascarada/presa, prioridade do vetor da IDT, IMCR, polaridade/modo de disparo, número de espaços de endereçamento em rotação a cada tick. O fator decisivo foi a frequência do timer: a 2 Hz e 20 Hz a entrega era perfeitamente confiável; só falhava a 100 Hz. **Confirmação definitiva**: rodando com aceleração de hardware real (`-accel kvm -cpu host`) em vez de TCG, a 100 Hz, toda a sequência de teclas foi entregue perfeitamente — prova que o driver está correto e a falha é específica da emulação por software do QEMU sob alta frequência de interrupção concorrente.

**Implicação prática:** verificação manual de drivers de IRQ externa via `cargo xtask run`/`sendkey` sob QEMU padrão (sem KVM) pode mostrar só a primeira interrupção simulada — isso é esperado, não indica regressão. Para verificar de verdade um driver de IRQ externa em boot manual, usar `-accel kvm` (requer `/dev/kvm` acessível) quando disponível.

## ~~Ausência de guard page na stack principal do kernel~~ (RESOLVIDO)

**Status:** resolvido pela change `kernel-stack-guard-page` (ver `openspec/changes/kernel-stack-guard-page/` — `design.md` para o racional completo).

**Descoberto em:** change `kernel-interrupts-timer`, tarefa 2.3 (verificação de double fault por estouro de stack).

**Contexto original:** o kernel rodava inteiramente sobre a stack que o Limine configura antes de entregar o controle em `kernel_main` — não havia nenhuma stack própria do kernel, nem uma guard page (região de memória deliberadamente não mapeada) logo abaixo dela.

**Sintoma observado:** ao provocar um estouro real de stack (recursão infinita), o kernel não falhava rapidamente como esperado. Em vez disso, a recursão sobrescrevia silenciosamente uma quantidade significativa de memória adjacente (sem causar nenhuma exceção de página) antes de eventualmente alcançar algo que gerava falha. Quando a falha finalmente ocorria e escalava para um double fault, o handler de double fault era corretamente invocado e executava na stack dedicada de IST1 (confirmado via GDB — `RSP` caía exatamente dentro do range de `DOUBLE_FAULT_STACK`), mas a tentativa de formatar e reportar a mensagem de diagnóstico travava indefinidamente (`RIP` ficava parado dentro de `core::fmt::write`), provavelmente devido a alguma estrutura de dados usada pela formatação ter sido corrompida durante o estouro.

**Correção implementada:** `kernel::stack` mapeia uma stack dedicada de 64 KiB numa faixa virtual isolada (`0xffff_a000_0000_0000`), com a página imediatamente abaixo dela deliberadamente não mapeada (guard page), e `kernel_main` troca para essa stack assim que o alocador de frames/paginação estão prontos. Durante a implementação, descobriu-se que o handler de `#PF` também precisava de sua própria stack via IST (mesmo mecanismo do `#DF`) — sem isso, um estouro de stack escalava direto para `#DF` sem o `#PF` nunca ser entregue, já que a CPU não conseguia empilhar o frame de exceção numa stack já invadida pela guard page (ver `design.md` — "Page fault também precisa de stack dedicada (IST)"). Com essa correção, um estouro real de stack agora produz `EXCECAO #PF (page fault)` com o endereço (`CR2`) dentro da guard page, verificado em boot real via QEMU/GDB.
