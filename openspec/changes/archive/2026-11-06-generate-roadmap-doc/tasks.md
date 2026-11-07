# Tasks

## 1. Levantamento do estado real

- [x] 1.1 Listar todas as changes arquivadas (`ls openspec/changes/archive`) e todas as capabilities atuais (`openspec list --specs`) — já feito nesta sessão: 24 changes arquivadas, 25 capabilities (incluindo `libc-runtime`, nova)
- [x] 1.2 Confirmar, lendo `docs/architecture/overview.md` (seções 1-6 e 15) e o histórico desta sessão, quais primitivas já foram verificadas em boot real (não só implementadas) — base para a seção "já feito" não listar nada que não tenha sido realmente comprovado rodando
- [x] 1.3 Comparar o estado real levantado acima com a seção "20. ROADMAP DE MODERNIZAÇÃO" de `SPECIFICATION.md` e registrar divergências encontradas: única divergência real — `FXSAVE`/`FXRSTOR` por thread está listado lá como pendência de "curto prazo", mas foi entregue pela change `kernel-fpu-context-switch` (arquivada na mesma sessão, depois de `SPECIFICATION.md` ter sido gerado). Demais itens de "curto/médio/longo prazo" dessa seção continuam de fato pendentes — usado na tarefa 4.1

## 2. Redigir "O que já foi feito"

- [x] 2.1 Escrever a seção organizada por área (kernel/boot, memória, processos/threads/escalonador, syscalls, HAL/drivers, FPU/ponto flutuante, libc C, carregador de executáveis/linker dinâmico), cada item citando a change que entregou e, quando aplicável, a verificação em boot real que a comprova — ver `ROADMAP.md`, "O que já está implementado"
- [x] 2.2 Verificar que cada item citado corresponde a uma change realmente arquivada (cruzar com a lista da tarefa 1.1): todas as 23 changes de sistema citadas em `ROADMAP.md` conferem com `ls openspec/changes/archive` — nenhuma inexistente ou não arquivada (a 24ª, `generate-system-specification`, não é uma feature de sistema, deliberadamente não citada)

## 3. Redigir "O que está previsto"

- [x] 3.1 Escrever a seção organizada por horizonte (próximo passo imediato / médio prazo / longo prazo), resumindo e referenciando — não duplicando por extenso — o roadmap completo de `docs/architecture/overview.md`, seção 15 — ver `ROADMAP.md`, "O que está previsto"
- [x] 3.2 Verificar que o "próximo passo imediato" citado é consistente com a última entrada real do roadmap em `docs/architecture/overview.md`: confirmado — esqueleto `os_javaos` do HotSpot em ambos os documentos

## 4. Seções em destaque: JVM e interface gráfica

- [x] 4.1 Escrever a seção "O que falta para a JVM rodar" — ver `ROADMAP.md`, "Destaque: o que falta para a JVM rodar" (esqueleto `os_javaos`, `libm`/conversão float, `libstdc++`/exceções C++, isolamento de falha por processo, código-fonte do HotSpot em si — nada citado aí já foi entregue, consistente com a tarefa 1.3)
- [x] 4.2 Escrever a seção "O que falta para uma interface gráfica" — ver `ROADMAP.md`, "Destaque: o que falta para uma interface gráfica" (primitivas de desenho via syscall, driver de mouse, compositor, toolkit, e a dependência estrutural do porte da JVM explicitada)

## 5. Montagem final e verificação

- [x] 5.1 Montar `ROADMAP.md` na raiz do repositório com as seções das tarefas 2-4, mais um cabeçalho curto explicando o propósito do documento e como ele se relaciona com `docs/architecture/overview.md`/`SPECIFICATION.md` (qual é a fonte detalhada de cada um) — arquivo criado
- [x] 5.2 Revisar o arquivo final: todas as alegações de "já feito" citam uma change arquivada real (ver tarefa 2.2); data de atualização registrada (2026-11-06) bate com a data real desta sessão

## Workflow follow-up

- Arquivar esta change (`opsx:archive`) depois que `ROADMAP.md` estiver escrito e revisado.
