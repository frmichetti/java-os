# Known Issues

## Ausência de guard page na stack principal do kernel

**Descoberto em:** change `kernel-interrupts-timer`, tarefa 2.3 (verificação de double fault por estouro de stack).

**Contexto:** o kernel atualmente roda sobre a stack que o Limine configura antes de entregar o controle em `kernel_main` — não há nenhuma stack própria do kernel, nem uma guard page (região de memória deliberadamente não mapeada) logo abaixo dela.

**Sintoma observado:** ao provocar um estouro real de stack (recursão infinita), o kernel não falha rapidamente como esperado. Em vez disso, a recursão sobrescreve silenciosamente uma quantidade significativa de memória adjacente (sem causar nenhuma exceção de página) antes de eventualmente alcançar algo que gera falha. Quando a falha finalmente ocorre e escala para um double fault, o handler de double fault é corretamente invocado e executa na stack dedicada de IST1 (confirmado via GDB — `RSP` cai exatamente dentro do range de `DOUBLE_FAULT_STACK`), mas a tentativa de formatar e reportar a mensagem de diagnóstico trava indefinidamente (`RIP` fica parado dentro de `core::fmt::write`), provavelmente devido a alguma estrutura de dados usada pela formatação ter sido corrompida durante o estouro.

**O que já foi verificado como correto** (ver `openspec/changes/archive/*-kernel-interrupts-timer/` quando arquivada): o mecanismo de double fault com IST funciona corretamente quando testado de forma controlada (invalidando temporariamente o handler de `#PF` para forçar uma escalada limpa para `#DF`, sem corromper memória) — nesse caso o diagnóstico é reportado corretamente e o kernel halta limpo, sem triple fault.

**Impacto:** um estouro real de stack durante operação normal do kernel pode corromper memória adjacente de forma silenciosa antes de ser detectado, e mesmo quando detectado (via double fault), o diagnóstico pode não ser reportado de forma confiável nesse cenário específico.

**Correção proposta (não implementada ainda):** criar uma stack própria do kernel (em vez de depender da do Limine), com uma guard page não mapeada logo abaixo dela, usando a capability `kernel-paging` (já implementada em `kernel-memory-management`) para mapear a stack e deixar a página adjacente deliberadamente ausente — assim um estouro de stack causa uma falha de página imediata e previsível, bem mais próxima do topo da stack, minimizando a corrupção colateral antes da falha ser detectada.
