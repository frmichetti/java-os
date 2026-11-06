# Design

## Context

O projeto é um kernel Rust `no_std` bare-metal x86_64 (boot UEFI/Limine), ainda
sem nenhuma aplicação de usuário real — apenas crates de teste
(`elf_test_program`, `elf_test_library_base/user`, `userland_c`) usadas para
validar o carregador ELF, o linker dinâmico e, mais recentemente, um port
mínimo de libc (change em andamento `libc-minimal-port`, 18/29 tasks). O
objetivo de longo prazo (ver `README.md`) é rodar aplicações Java sobre este
kernel.

O conhecimento do sistema hoje vive em três camadas que não se sobrepõem
totalmente:

1. **Código-fonte** (`kernel/src/*.rs`, ~50 arquivos) — a fonte da verdade,
   por instrução de `CLAUDE.md` e do prompt de origem.
2. **22 changes arquivadas** em `openspec/changes/archive/` (cada uma com
   `proposal.md`/`design.md`/`tasks.md`) e **25 capabilities** em
   `openspec/specs/<nome>/spec.md` — registram requisitos e decisões de design
   por incremento, mas nenhuma dá a visão consolidada do sistema como um todo.
3. **`docs/architecture/overview.md`** (655 linhas, 15 áreas) — já é uma
   síntese incremental, mantida a cada change, mas segue uma estrutura própria
   (por área técnica do kernel), não o template de 25 seções do SDD pedido
   pelo usuário, e não cobre regras de negócio/casos de uso/critérios de
   aceitação no formato do template.

O template-fonte (`Specification-Driven Development.pdf`, 25 seções) foi
escrito para uma aplicação web CRUD típica (REST, banco relacional, telas,
OAuth2/SSO, filas de mensageria). Boa parte das seções não tem equivalente
direto em um kernel bare-metal. O arquivo `Você é um Arquiteto de Software.md`
fornecido pelo usuário está incompleto: detalha por extenso apenas a seção 1
("Visão Geral do Sistema") e é cortado no meio de um bloco de exemplo Mermaid,
sem chegar a enumerar as seções 2+ — mas a "REGRA FINAL" do PDF (página 15)
deixa claro o padrão exigido para todo o documento: detalhe suficiente para
recriar banco de dados, backend, frontend, integrações, testes, suportar
migração/refatoração e ser consumível por um agente de IA como fonte única da
verdade.

## Goals / Non-Goals

**Goals:**
- Definir o mapeamento seção-a-seção entre o template de 25 seções do PDF e a
  realidade do projeto, para que a tarefa de escrita (fase de implementação)
  tenha um roteiro claro por seção em vez de reinterpretar o template do zero.
- Definir a estratégia de consolidação de fontes (código > specs arquivadas >
  `docs/architecture/overview.md` > README/KNOW_ISSUES) e como tratar
  divergências entre elas.
- Definir como marcar e justificar seções do template sem equivalente no
  domínio (em vez de preenchê-las com conteúdo genérico ou inventado).
- Definir a estratégia de diagramas Mermaid (quais seções recebem diagrama e
  que tipo).
- Garantir que a change em andamento (`libc-minimal-port`, não arquivada, não
  refletida em `docs/architecture/overview.md`) seja tratada corretamente:
  como estado "em andamento", não como concluída.

**Non-Goals:**
- Esta change não decide *conteúdo de negócio* novo — não inventa requisitos,
  regras ou telas que não existam no código ou nas specs. Onde o template pede
  algo que o sistema não tem (ex.: telas, autenticação JWT/OAuth2), a seção é
  marcada como não aplicável com justificativa, não preenchida com suposições.
- Não revisa nem corrige o código-fonte, mesmo que ambiguidades ou divergências
  sejam encontradas — apenas as documenta (ver `CLAUDE.md`: correções viram
  entrada em `KNOW_ISSUES.md`, não edição direta).
- Não substitui nem apaga `docs/architecture/overview.md` nem as specs
  arquivadas — `SPECIFICATION.md` é um novo documento de síntese que pode
  referenciá-los, não uma migração destrutiva.

## Decisions

### 1. Mapeamento das 25 seções do template para o domínio do kernel

O template do PDF é genérico (app web). Decisão: manter a numeração e os
títulos das 25 seções (para rastreabilidade com o template-fonte), mas
reinterpretar o conteúdo de cada uma para o domínio de SO, e marcar
explicitamente "Não aplicável" quando não houver equivalente, com uma frase
justificando o porquê (ex.: "Não aplicável — o sistema não expõe UI gráfica de
aplicação; ver seção 1 do `docs/architecture/overview.md`, framebuffer console
é infraestrutura de kernel, não uma tela de usuário"). Alternativa descartada:
remover seções sem equivalente — rejeitada porque quebra a rastreabilidade com
o template pedido pelo usuário e com a "regra final" do PDF (lista de
capacidades que o documento deve suportar, incluindo migração/refatoração,
onde "não aplicável, e por quê" também é informação útil).

Mapeamento adotado (para orientar `tasks.md`):

| # | Seção do template | Equivalente no projeto |
|---|---|---|
| 1 | Visão Geral do Sistema | Objetivo (README.md), problema (porte de JVM/Java apps sobre kernel próprio), público-alvo (equipe do projeto/futuros devs Java OS), fluxo geral (boot → kernel → processos/syscalls → I/O) |
| 2 | Arquitetura | `docs/architecture/overview.md` seções 1–5, 12–14 (estilo: kernel monolítico modular; não é web app em camadas) |
| 3 | Regras de Negócio | Invariantes/políticas do kernel com força de regra de negócio (ex.: layout fixo de VA por região, FAT32 só leitura, nomes 8.3, `--hash-style=sysv` obrigatório, isolamento de endereço por `CR3`) — formato BR-XXX adaptado |
| 4 | Casos de Uso | Fluxos operacionais do kernel (boot completo, spawn de processo via ELF, `dlopen`/`dlsym`, leitura de arquivo, troca de contexto por timer) como casos de uso técnicos, não de usuário final de aplicação |
| 5 | Modelo de Domínio | Estruturas centrais do kernel: `Process`, `Thread`, `Region` (vmm), `FrameAllocator`, handles (`file_handles`, `pthread_handles`) — "entidades" em vez de modelo relacional |
| 6 | Banco de Dados | Não há SGBD. Equivalente real: layout on-disk (GPT + partição EFI FAT32), estruturas FAT32 lidas (`kernel::fat32`/`kernel::gpt`) — documentar como "armazenamento persistente", não como seção de BD relacional |
| 7 | APIs | Tabela de syscalls (`kernel::syscall`, ABI número+4 args+retorno) — o equivalente real de "endpoint" é um número de syscall |
| 8 | Telas | Não aplicável (sem UI de aplicação) — mencionar console de framebuffer/serial (`kernel::framebuffer`) como infraestrutura, não como tela |
| 9 | Segurança | Anel 0 vs. anel 3, `syscall`/`sysret`, isolamento de espaço de endereçamento por processo, guard pages — sem JWT/OAuth2/SSO (não aplicável) |
| 10 | Integrações | Hardware/protocolos reais: PCI (`kernel::pci`), AHCI (`kernel::storage`), e1000 (`kernel::network`), ACPI/Limine boot protocol — não REST/Kafka/RabbitMQ |
| 11 | Processos Assíncronos | Escalonador preemptivo (`kernel::scheduler`), threads e sincronização (`kernel::pthread`, `kernel::wait_queue`) — não jobs agendados |
| 12 | Configurações | `rust-toolchain.toml`, `x86_64-java_os*.json` (target specs), `.cargo/config.toml`, variáveis de build do `xtask` |
| 13 | Logs e Auditoria | Saída serial/framebuffer (`kernel::serial`), sem auditoria formal — registrar o que existe e marcar o resto como gap |
| 14 | Requisitos Funcionais | Extraídos das 25 capabilities em `openspec/specs/` — cada capability vira um RF-XXX com referência cruzada |
| 15 | Requisitos Não Funcionais | Extraídos de decisões de design recorrentes (sem alocação dinâmica além do heap controlado, `no_std`, timing calibrado por PIT, etc.) |
| 16 | Critérios de Aceitação | Smoke tests existentes (`cargo xtask test`) e critérios de boot real documentados nas changes arquivadas |
| 17 | Testes | `docs/architecture/overview.md` seção 14 (estratégia de testes) |
| 18 | Observabilidade | Saída serial + relato de boot/memória (`kernel::memmap_report`) — sem métricas/dashboards (não aplicável) |
| 19 | Dívida Técnica | Consolidar `KNOW_ISSUES.md` + gaps explicitamente citados nas specs arquivadas (ex.: SMP adiado, huge pages futuras, isolamento de falhas por processo pendente) |
| 20 | Roadmap de Modernização | `docs/architecture/overview.md` seção 15 (roadmap completo) |
| 21 | Matriz de Rastreabilidade | RF (seção 14) × capability (`openspec/specs/`) × arquivo-fonte × syscall, adaptada (sem Tela/API REST) |
| 22 | Glossário | Termos específicos de OS dev usados no projeto (HHDM, PML4, GDT/IDT/TSS, ET_DYN, DT_NEEDED, etc.) |
| 23 | Anexos | Referências para `docs/architecture/overview.md`, PDF de origem, changes arquivadas |
| 24 | Histórico de Versões | Entrada inicial desta geração (data, autor = esta change) |
| 25 | Aprovações | Não aplicável — projeto não tem processo formal de aprovação documentado; registrar como tal |

### 2. Ordem de precedência das fontes

Ordem de autoridade ao escrever cada seção: **código-fonte atual** > **spec
arquivada/capability relacionada** > **`docs/architecture/overview.md`** >
`README.md`/`KNOW_ISSUES.md`. Qualquer divergência encontrada entre essas
camadas (ex.: uma spec arquivada descreve um comportamento que o código atual
não tem mais, ou vice-versa) é registrada na seção 19 (Dívida Técnica) ou como
nota inline, nunca silenciosamente resolvida a favor de uma fonte — por
instrução de `CLAUDE.md` ("em caso de conflito... informe qualquer
inconsistência encontrada").

### 3. Tratamento da change em andamento (`libc-minimal-port`)

Decisão: tratar como "em andamento" explicitamente em toda seção que ela
toque (userland C mínimo, `crt0.S`/`glue.c`), citando o estado real
(18/29 tasks) em vez de omitir ou tratar como concluída — evita que
`SPECIFICATION.md` fique desatualizado no dia em que for gerado.

### 4. Diagramas Mermaid

Incluir ao menos: (a) `flowchart` do fluxo operacional geral (seção 1); (b)
`flowchart` de camadas do kernel (seção 2, baseado nas 15 áreas do
`overview.md`); (c) `sequenceDiagram` do ciclo de uma syscall (modo usuário →
`syscall`/`sysret` → handler → retorno); (d) `sequenceDiagram` do carregamento
de um executável ELF com dependências dinâmicas (`spawn_from_elf` →
`dlopen`/`dlsym` recursivo). Diagramas adicionais ficam a critério da fase de
escrita onde agregarem clareza, mas esses quatro são obrigatórios por
cobrirem os fluxos mais citados nas changes arquivadas.

### 5. Granularidade de leitura do código-fonte

Dado o volume (~50 arquivos em `kernel/src/`, mais `userland_c/`,
`elf_test_program/`, `xtask/`), a fase de escrita deve ler integralmente os
arquivos citados como fonte primária de cada seção (não apenas os nomes),
já que o prompt de origem proíbe resumir/simplificar/omitir detalhes.
Arquivos `*_demo.rs` (smoke tests de boot real) são lidos para confirmar
comportamento verificado, não para arquitetura.

## Risks / Trade-offs

- **[Risco]** O documento final pode ficar muito extenso (síntese de 50+
  arquivos de código, 22 changes, 25 specs) e difícil de manter.
  **Mitigação:** manter `docs/architecture/overview.md` como documento vivo
  de arquitetura técnica (não duplicado, apenas referenciado por
  `SPECIFICATION.md`); `SPECIFICATION.md` foca em consolidar a visão completa
  no formato SDD pedido, mas pode referenciar o overview para detalhe técnico
  extenso em vez de copiá-lo integralmente.
- **[Risco]** Seções marcadas "não aplicável" podem ser mal interpretadas no
  futuro como lacunas não investigadas. **Mitigação:** cada "não aplicável"
  vem com justificativa de uma frase, nunca em branco.
- **[Risco]** `SPECIFICATION.md` pode ficar desatualizado assim que a próxima
  change for implementada. **Mitigação:** fora do escopo desta change, mas
  `CLAUDE.md` já institui que toda manutenção futura deve manter a
  documentação atualizada — registrar isso na seção 24 (Histórico de Versões)
  do próprio documento gerado.

## Open Questions

Nenhuma — as únicas ambiguidades identificadas (arquivo-fonte incompleto,
seções do template sem equivalente no domínio, estado da change em
andamento) já têm decisão registrada acima e serão documentadas, não
resolvidas por suposição, dentro do próprio `SPECIFICATION.md`.
