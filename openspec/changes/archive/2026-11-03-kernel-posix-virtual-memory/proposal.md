# Proposal

## Why

A change `jvm-posix-compat-strategy` (arquivada) inventariou as primitivas POSIX mínimas exigidas pelo HotSpot e identificou a memória virtual como um dos gaps estruturais: o kernel hoje só tem `kernel::paging::Mapper::map`/`map_new` (página já resolvida, sempre com frame físico comprometido na hora do mapeamento) — **não existe nenhum conceito de "reservado mas não committado"** (reservar uma faixa de endereço virtual grande sem gastar memória física até que partes dela sejam efetivamente usadas), nem forma de alterar as permissões de uma página já mapeada sem desmapeá-la primeiro. Essas duas capacidades juntas são a base de `mmap`/`munmap`/`mprotect` POSIX, que o HotSpot usa para o heap Java, o *code cache* do JIT e páginas de guarda — reservando uma faixa grande de uma vez e comprometendo/protegendo sub-regiões dela ao longo da execução.

Esta é a segunda sub-change do item 8 do roadmap ("Porte inicial da JVM"), escolhida por ser independente da primeira (`kernel-wait-queue`, ainda em andamento — `pthread_mutex`/`pthread_cond` dependem dela, mas memória virtual não).

## What Changes

- Acrescenta a `kernel::paging::Mapper` a capacidade de alterar as permissões (`PageFlags`) de uma página já mapeada, mantendo o mesmo frame físico, sem exigir desmapear e remapear (primitiva de nível de página por trás de `mprotect`).
- Introduz um novo módulo `kernel::vmm` (virtual memory manager) sobre uma faixa virtual fixa reservada (padrão já usado por `kernel::heap`/`kernel::stack`), que permite:
  - **Reservar** uma faixa de endereço virtual de tamanho arbitrário sem comprometer memória física (`reserve`, análogo a `mmap(PROT_NONE)`/`mmap(MAP_NORESERVE)`);
  - **Comprometer** sob demanda uma sub-região de uma reserva, mapeando frames físicos zerados (`commit`, análogo a tornar uma sub-faixa de `mmap` efetivamente utilizável);
  - **Descomprometer** uma sub-região já comprometida, liberando os frames físicos de volta ao alocador mas preservando a reserva virtual para um `commit` futuro (`decommit`, análogo a `madvise(MADV_DONTNEED)`);
  - **Alterar permissões** de uma sub-região já comprometida (`protect`, usa a nova primitiva de `Mapper`, análogo a `mprotect`);
  - **Liberar** uma reserva inteira, descomprometendo o que ainda estiver comprometido e devolvendo a faixa virtual para reaproveitamento por reservas futuras (`release`, análogo a `munmap`).
- Um demo em modo kernel (`kernel::vmm_demo`, análogo a `wait_demo`/`keyboard_demo`) prova o ciclo completo: reservar uma faixa grande, comprometer e escrever numa sub-região, alterar sua proteção para somente-leitura e confirmar (via page fault esperado) que uma escrita subsequente é rejeitada, descomprometer e liberar.
- **Não inclui ainda** (fica para sub-changes futuras do item 8): as chamadas `mmap`/`munmap`/`mprotect` como *syscalls* nomeadas expostas a modo usuário, qualquer integração com `kernel::process::Process` (regiões por processo em vez de uma faixa única), paginação sob demanda disparada por *page fault* (o `commit` desta change é sempre explícito, nunca automático a partir de um handler de exceção), e qualquer código do HotSpot/OpenJDK em si.
- Nenhuma mudança de comportamento observável para o código já existente (heap, stack, processos, teclado, armazenamento, rede, modo usuário) — `kernel::heap`/`kernel::stack` continuam usando `Mapper::map`/`map_new` diretamente, sem passar pelo novo `kernel::vmm`.

## Capabilities

### New Capabilities
- `kernel-virtual-memory`: reserva, comprometimento, descomprometimento, alteração de proteção e liberação de faixas de memória virtual anônima, com reaproveitamento de faixas liberadas — a base de `mmap`/`munmap`/`mprotect` POSIX.

### Modified Capabilities
- `kernel-paging`: adiciona o requisito de que uma página já mapeada pode ter suas permissões de acesso alteradas sem trocar o frame físico mapeado e sem passar por um desmapeamento intermediário (primitiva de nível de página usada por `kernel-virtual-memory` para `protect`).

## Impact

- `kernel/src/paging.rs`: `Mapper` ganha um método `protect` (ou equivalente) que reescreve as flags de uma `PageTableEntry` já presente, preservando o endereço físico, e invalida a TLB da página afetada.
- Novo módulo `kernel::vmm` (a primitiva de reserva/commit/decommit/protect/release em si, com a lógica pura de rastreamento de faixas — tamanhos, alinhamento, sobreposição, reaproveitamento — testável em host) e `kernel::vmm_demo` (a demonstração em modo kernel).
- `kernel/src/main.rs`: `continue_boot` ganha o spawn da thread de demonstração de `kernel::vmm_demo`, no mesmo ponto onde as demos existentes já são iniciadas.
- Nenhuma mudança em `kernel::heap`, `kernel::stack`, `kernel::process` ou `kernel::hal` (esta primitiva não é um dispositivo físico e ainda não se integra ao ciclo de vida de processos).
- `docs/architecture/overview.md`: seção 2 (Memória) e o item 8 do roadmap (seção 15) ganham uma atualização registrando esta segunda sub-change concluída.
