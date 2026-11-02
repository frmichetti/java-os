# Known Issues

## ~~Ausência de guard page na stack principal do kernel~~ (RESOLVIDO)

**Status:** resolvido pela change `kernel-stack-guard-page` (ver `openspec/changes/kernel-stack-guard-page/` — `design.md` para o racional completo).

**Descoberto em:** change `kernel-interrupts-timer`, tarefa 2.3 (verificação de double fault por estouro de stack).

**Contexto original:** o kernel rodava inteiramente sobre a stack que o Limine configura antes de entregar o controle em `kernel_main` — não havia nenhuma stack própria do kernel, nem uma guard page (região de memória deliberadamente não mapeada) logo abaixo dela.

**Sintoma observado:** ao provocar um estouro real de stack (recursão infinita), o kernel não falhava rapidamente como esperado. Em vez disso, a recursão sobrescrevia silenciosamente uma quantidade significativa de memória adjacente (sem causar nenhuma exceção de página) antes de eventualmente alcançar algo que gerava falha. Quando a falha finalmente ocorria e escalava para um double fault, o handler de double fault era corretamente invocado e executava na stack dedicada de IST1 (confirmado via GDB — `RSP` caía exatamente dentro do range de `DOUBLE_FAULT_STACK`), mas a tentativa de formatar e reportar a mensagem de diagnóstico travava indefinidamente (`RIP` ficava parado dentro de `core::fmt::write`), provavelmente devido a alguma estrutura de dados usada pela formatação ter sido corrompida durante o estouro.

**Correção implementada:** `kernel::stack` mapeia uma stack dedicada de 64 KiB numa faixa virtual isolada (`0xffff_a000_0000_0000`), com a página imediatamente abaixo dela deliberadamente não mapeada (guard page), e `kernel_main` troca para essa stack assim que o alocador de frames/paginação estão prontos. Durante a implementação, descobriu-se que o handler de `#PF` também precisava de sua própria stack via IST (mesmo mecanismo do `#DF`) — sem isso, um estouro de stack escalava direto para `#DF` sem o `#PF` nunca ser entregue, já que a CPU não conseguia empilhar o frame de exceção numa stack já invadida pela guard page (ver `design.md` — "Page fault também precisa de stack dedicada (IST)"). Com essa correção, um estouro real de stack agora produz `EXCECAO #PF (page fault)` com o endereço (`CR2`) dentro da guard page, verificado em boot real via QEMU/GDB.
