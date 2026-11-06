# Tasks

## 1. Preparação

- [x] 1.1 Criar `SPECIFICATION.md` na raiz com o esqueleto das 25 seções do
      template (títulos e numeração conforme a tabela de mapeamento do
      `design.md`), sem conteúdo ainda, e verificar que os 25 títulos estão
      presentes e na ordem do template.
- [x] 1.2 Ler integralmente as 22 changes em `openspec/changes/archive/*`
      (proposal.md + design.md de cada uma) e as 25 capabilities em
      `openspec/specs/*/spec.md`, listando por escrito (notas de trabalho,
      não commitadas) qual arquivo-fonte de `kernel/src/` cada uma cobre —
      verificar cruzando a lista contra `ls kernel/src/*.rs` que nenhum
      arquivo relevante ficou sem change/spec associada.
- [x] 1.3 Ler integralmente `README.md`, `KNOW_ISSUES.md`, `docs/architecture/overview.md`
      (todas as 655 linhas) e o estado atual de `openspec/changes/libc-minimal-port/`
      (proposal.md, design.md, tasks.md) para confirmar o que já está
      concluído vs. em andamento — verificar anotando explicitamente o
      número de tasks concluídas/total da change em andamento.

## 2. Seção 1 — Visão Geral do Sistema

- [x] 2.1 Escrever 1.1 (Objetivo, problema de negócio, público-alvo,
      benefícios) a partir de `README.md` e da seção 15 (Roadmap) de
      `docs/architecture/overview.md` — verificar que cada afirmação cita o
      arquivo-fonte de onde veio.
- [x] 2.2 Escrever 1.2 (Escopo incluído/não incluído) distinguindo o que está
      implementado hoje (código real, changes arquivadas) do que é roadmap
      futuro (seções 6–11 do `overview.md`, ainda não implementadas) —
      verificar que nenhum item de "Incluído" descreve algo apenas planejado.
- [x] 2.3 Escrever 1.3 (Fluxo Geral) com um diagrama Mermaid `flowchart`
      cobrindo boot → inicialização de subsistemas → carregamento/execução de
      processo → syscalls/I/O, conforme decisão de diagramas do `design.md`
      — verificar que o diagrama usa sintaxe Mermaid válida (sem erros de
      sintaxe ao inspecionar manualmente contra a referência da linguagem).

## 3. Seção 2 — Arquitetura

- [x] 3.1 Escrever 2.1 (Visão Arquitetural: estilo = kernel monolítico
      modular, não microsserviços/web) e 2.2 (diagrama Mermaid de camadas:
      boot/HAL → gerência de memória → processos/escalonador → syscalls →
      drivers/FS/rede) a partir das seções 1–5 do `overview.md` — verificar
      que o diagrama reflete as mesmas camadas citadas no texto.
- [x] 3.2 Escrever 2.3 (Estrutura de Módulos) listando cada módulo relevante
      de `kernel/src/*.rs` com descrição, responsabilidades e dependências
      (ex.: `scheduler` depende de `process`, `stack`, `paging`) — verificar
      contra `kernel/src/lib.rs`/`main.rs` que todo módulo público citado
      existe de fato no código.

## 4. Seções 3–5 — Regras de Negócio, Casos de Uso, Modelo de Domínio

- [x] 4.1 Escrever seção 3 (Regras de Negócio) no formato BR-XXX, extraindo
      invariantes com força de regra do código e das specs arquivadas (ex.:
      layout fixo de índices de PML4 por região, FAT32 somente leitura e
      nomes 8.3, `--hash-style=sysv` obrigatório no linker dinâmico,
      isolamento de guard page abaixo da stack) — verificar que cada BR-XXX
      aponta arquivo/função real em `kernel/src/`.
- [x] 4.2 Escrever seção 4 (Casos de Uso) no formato UC-XXX para os fluxos
      operacionais principais (boot completo, spawn de processo via ELF
      estático, `dlopen`/`dlsym` de biblioteca dinâmica, abrir/ler/fechar
      arquivo via syscall, troca de contexto disparada pelo timer) —
      verificar que cada UC-XXX tem pré-condição, fluxo principal e ao menos
      um fluxo de exceção extraído de comportamento real (ex.: double fault,
      falta de página fora de região registrada).
- [x] 4.3 Escrever seção 5 (Modelo de Domínio) documentando as estruturas
      centrais (`Process`, `Thread`, `Region`, `FrameAllocator`, handles de
      arquivo/mutex/condvar) com campos e relacionamentos — verificar contra
      as `struct`/`enum` reais em `kernel/src/process.rs`, `kernel/src/vmm.rs`,
      `kernel/src/file_handles.rs`, `kernel/src/pthread_handles.rs`.

## 5. Seções 6–7 — Armazenamento e Interfaces (equivalentes de BD/API)

- [x] 5.1 Escrever seção 6 (Banco de Dados → armazenamento persistente real):
      layout GPT + partição EFI FAT32, estruturas lidas por `kernel::gpt`/
      `kernel::fat32`, limitações (somente leitura, nomes 8.3) — verificar
      contra `kernel/src/gpt.rs` e `kernel/src/fat32.rs`, e marcar
      explicitamente as subsções do template sem equivalente (View/Sequence/
      Procedure) como não aplicável com justificativa.
- [x] 5.2 Escrever seção 7 (APIs → tabela de syscalls) listando cada número
      de syscall em `kernel::syscall`, seus argumentos, retorno, e a regra de
      negócio/capability associada — incluir um `sequenceDiagram` Mermaid do
      ciclo de uma syscall (modo usuário → `syscall`/`sysret` → handler →
      retorno), conforme decisão do `design.md` — verificar contando que o
      número de entradas na tabela bate com o número de `match`/constantes
      `SYSCALL_*` em `kernel/src/syscall.rs`.

## 6. Seções 8–11 — Interface, Segurança, Integrações, Execução Concorrente

- [x] 6.1 Escrever seção 8 (Telas) marcando como não aplicável (sem UI de
      aplicação) e documentando o console de framebuffer/serial
      (`kernel::framebuffer`, `kernel::serial`) como infraestrutura — verificar
      que a justificativa de não-aplicabilidade está presente.
- [x] 6.2 Escrever seção 9 (Segurança): anel 0 vs. anel 3, mecanismo
      `syscall`/`sysret` (STAR/LSTAR/SFMASK, EFER.SCE), isolamento de espaço
      de endereçamento por processo (`CR3` próprio), guard pages — marcando
      JWT/OAuth2/SSO como não aplicável — verificar contra `kernel/src/gdt.rs`
      e a seção 4 do `overview.md`.
- [x] 6.3 Escrever seção 10 (Integrações → hardware/protocolos reais): PCI
      (`kernel::pci`), AHCI (`kernel::storage`), e1000 (`kernel::network`),
      protocolo de boot Limine/ACPI — verificar contra `kernel/src/pci.rs`,
      `kernel/src/storage.rs`, `kernel/src/network.rs`.
- [x] 6.4 Escrever seção 11 (Processos Assíncronos → escalonador/threads):
      `kernel::scheduler` (round-robin preemptivo disparado pelo timer),
      `kernel::pthread` (mutex/condvar nomeados), `kernel::wait_queue`
      (bloqueio/despertar voluntário) — incluir um `sequenceDiagram` Mermaid
      do carregamento de um executável ELF com dependência dinâmica
      (`spawn_from_elf` → `dlopen` recursivo → `dlsym` → chamada resolvida),
      conforme decisão do `design.md` — verificar contra
      `kernel/src/scheduler.rs` e `kernel/src/dylib.rs`.

## 7. Seções 12–15 — Configuração e Requisitos

- [x] 7.1 Escrever seção 12 (Configurações): `rust-toolchain.toml`,
      `x86_64-java_os*.json` (três target specs), `.cargo/config.toml`,
      flags de build do `xtask` — verificar que cada arquivo citado existe na
      raiz do repositório.
- [x] 7.2 Escrever seção 13 (Logs e Auditoria): saída serial/framebuffer
      existente, e registrar explicitamente a ausência de auditoria formal
      como gap (não preencher com suposição) — verificar contra
      `kernel/src/serial.rs`.
- [x] 7.3 Escrever seção 14 (Requisitos Funcionais) derivando um RF-XXX por
      capability em `openspec/specs/` (25 capabilities) com referência
      cruzada ao caminho da spec — verificar que o número de RF-XXX é igual
      ao número de diretórios em `openspec/specs/`.
- [x] 7.4 Escrever seção 15 (Requisitos Não Funcionais) a partir de decisões
      de design recorrentes nas changes arquivadas (restrições `no_std`,
      calibração de timing via PIT 8254, orçamento de heap de 256 MiB,
      ausência de SMP) — verificar que cada RNF-XXX cita a change/arquivo de
      origem da decisão.

## 8. Seções 16–18 — Aceitação, Testes, Observabilidade

- [x] 8.1 Escrever seção 16 (Critérios de Aceitação) no formato
      Gherkin/Mermaid-livre a partir dos smoke tests reais (`cargo xtask
      test`, verificações de boot real citadas nas changes arquivadas, ex.:
      `kernel::clock_demo`, `kernel::vmm_demo`) — verificar que cada cenário
      corresponde a um `*_demo.rs` ou teste existente em `kernel/src/`.
- [x] 8.2 Escrever seção 17 (Testes) resumindo a seção 14 do
      `docs/architecture/overview.md` (testes de caixa-preta via QEMU, testes
      unitários no host) sem duplicar o texto, apenas referenciando-o e
      acrescentando o que for específico do formato do template — verificar
      que a seção linka explicitamente para `docs/architecture/overview.md`.
- [x] 8.3 Escrever seção 18 (Observabilidade): relato de boot/mapa de memória
      (`kernel::memmap_report`) e saída serial como os únicos mecanismos
      existentes, marcando métricas/dashboards (Grafana/Kibana/Prometheus)
      como não aplicável — verificar contra `kernel/src/memmap_report.rs`.

## 9. Seções 19–20 — Dívida Técnica e Roadmap

- [x] 9.1 Escrever seção 19 (Dívida Técnica) consolidando `KNOW_ISSUES.md`
      (lido na íntegra na task 1.3) com gaps explicitamente citados nas
      changes arquivadas (SMP adiado, huge pages futuras, isolamento de
      falhas por processo pendente, auditoria/observabilidade ausentes) e
      qualquer divergência código-vs-spec encontrada durante a escrita das
      seções anteriores — verificar que cada item cita severidade e
      arquivo/change de origem.
- [x] 9.2 Escrever seção 20 (Roadmap de Modernização) reaproveitando a seção
      15 (Roadmap Completo) de `docs/architecture/overview.md`, reorganizada
      em curto/médio/longo prazo — verificar que todo item do roadmap do
      `overview.md` aparece em algum horizonte.

## 10. Seções 21–25 — Rastreabilidade, Glossário, Anexos, Histórico, Aprovações

- [x] 10.1 Escrever seção 21 (Matriz de Rastreabilidade) cruzando RF (seção
      14) × capability (`openspec/specs/`) × arquivo-fonte × syscall
      associada, no formato de tabela adaptado (sem colunas Tela/API REST) —
      verificar que todo RF-XXX da seção 14 aparece ao menos uma vez na
      matriz.
- [x] 10.2 Escrever seção 22 (Glossário) com os termos de OS dev usados ao
      longo do documento (HHDM, PML4, GDT/IDT/TSS, ET_DYN, DT_NEEDED, IST,
      x2APIC, etc.) — verificar que todo termo em caixa alta/sigla citado nas
      seções 1–21 aparece no glossário.
- [x] 10.3 Escrever seção 23 (Anexos) referenciando
      `docs/architecture/overview.md`, `Specification-Driven Development.pdf`,
      e o diretório `openspec/changes/archive/` — seção 24 (Histórico de
      Versões) com uma única entrada inicial (versão 1.0, data de hoje,
      autor = esta change) — seção 25 (Aprovações) marcada como não
      aplicável (projeto não tem processo formal de aprovação documentado) —
      verificar que as três seções estão presentes e preenchidas ou
      justificadas.

## 11. Revisão final

- [x] 11.1 Reler `SPECIFICATION.md` do início ao fim e confirmar que nenhuma
      das 25 seções ficou vazia sem uma justificativa explícita de "não
      aplicável", e que todo diagrama Mermaid tem sintaxe válida — verificar
      lendo o arquivo completo em uma única passada.
- [x] 11.2 Conferir que toda ambiguidade e todo comportamento implícito
      identificados durante a escrita (incluindo a incompletude do próprio
      arquivo `Você é um Arquiteto de Software.md`, registrada no `design.md`)
      estão documentados explicitamente no corpo de `SPECIFICATION.md` — não
      apenas nesta change — verificar por busca textual de marcações de
      ambiguidade dentro do arquivo gerado.
- [x] 11.3 Atualizar `CLAUDE.md` se necessário para confirmar que a referência
      a `SPECIFICATION.md` como fonte única da verdade continua correta
      agora que o arquivo existe — verificar lendo a seção "Fonte da Verdade"
      de `CLAUDE.md` após a criação do documento.
