import { isAppLanguage, t, type AppLanguage, type I18nKey } from './vendor/markra/shared/i18n';
import { localizedWorkflowCopy, traditionalWorkflowCopy, workflowCopy, workflowLocales } from './workflow-copy';

const languages = ['zh-CN', 'en', 'zh-TW', 'ja', 'ko', 'fr', 'de', 'es', 'pt-BR', 'it', 'ru'] as const satisfies readonly AppLanguage[];
type MessageRow<Codes extends readonly string[]> = { readonly [Index in keyof Codes]: string };

// QingYu terms that have no equivalent in the Markra dictionaries.
const messages = {
  libraryRoot: ['笔记库顶层', 'Library Root', '筆記庫頂層', 'ライブラリの最上位', '보관함 최상위', 'Racine de la bibliothèque', 'Oberste Bibliotheksebene', 'Raíz de la biblioteca', 'Raiz da biblioteca', 'Radice della raccolta', 'Корень библиотеки'],
  memoFolderReserved: ['此名称已保留给随记使用', 'This name is reserved for Jots', '此名稱已保留給隨記使用', 'この名前はメモ用に予約されています', '이 이름은 한줄 메모용으로 예약되어 있습니다', 'Ce nom est réservé aux notes rapides', 'Dieser Name ist für Kurznotizen reserviert', 'Este nombre está reservado para las notas rápidas', 'Este nome está reservado para notas rápidas', 'Questo nome è riservato agli appunti veloci', 'Это имя зарезервировано для быстрых заметок'],
  memoFolderDestination: ['不能把文件夹放到随记目录中', 'Folders cannot be placed inside Jots', '不能把資料夾放到隨記目錄中', 'メモの中にフォルダを置くことはできません', '한줄 메모 안에 폴더를 넣을 수 없습니다', 'Impossible de placer un dossier dans les notes rapides', 'Ordner können nicht in Kurznotizen abgelegt werden', 'No puedes colocar carpetas en las notas rápidas', 'Não é possível colocar pastas nas notas rápidas', 'Non puoi inserire cartelle negli appunti veloci', 'Папки нельзя помещать в быстрые заметки'],
  memoAttachmentsUnavailable: ["当前笔记库不支持附件，请打开本地或服务器笔记库后重试","Open a local or server library to add attachments.","目前的筆記庫不支援附件，請開啟本機或伺服器筆記庫後重試","添付ファイルを追加するにはローカルまたはサーバーのライブラリを開いてください","첨부 파일을 추가하려면 로컬 또는 서버 보관함을 여세요","Ouvrez une bibliothèque locale ou sur serveur pour ajouter des pièces jointes.","Öffne eine lokale oder serverbasierte Bibliothek, um Anhänge hinzuzufügen.","Abre una biblioteca local o de servidor para añadir adjuntos.","Abra uma biblioteca local ou de servidor para adicionar anexos.","Apri una raccolta locale o su server per aggiungere allegati.","Откройте локальную или серверную библиотеку, чтобы добавить вложения."],
  memoRetryAttachments: ["重试附件","Retry Attachments","重試附件","添付ファイルを再試行","첨부 파일 다시 시도","Réessayer les pièces jointes","Anhänge erneut versuchen","Reintentar adjuntos","Tentar anexos novamente","Riprova con gli allegati","Повторить загрузку вложений"],
  memoResolveAttachments: ["请先重试或移除未保存的附件","Retry or remove the unsaved attachments first.","請先重試或移除未儲存的附件","保存できなかった添付ファイルを再試行するか削除してください","저장되지 않은 첨부 파일을 다시 시도하거나 삭제하세요","Réessayez ou retirez les pièces jointes non enregistrées.","Versuche es erneut oder entferne die nicht gespeicherten Anhänge.","Reintenta o elimina los adjuntos sin guardar.","Tente novamente ou remova os anexos não salvos.","Riprova o rimuovi gli allegati non salvati.","Повторите загрузку несохранённых вложений или удалите их."],
  memoAttachmentLimit: ["单个附件最多 64 MiB，每次最多 256 MiB","Attachments are limited to 64 MiB each and 256 MiB per batch.","單個附件最多 64 MiB，每次最多 256 MiB","添付ファイルは 1 件 64 MiB、合計 256 MiB までです","첨부 파일은 개당 64 MiB, 한 번에 256 MiB까지 추가할 수 있습니다","64 Mio par pièce jointe, 256 Mio par envoi.","Höchstens 64 MiB pro Anhang und 256 MiB pro Upload.","Hasta 64 MiB por adjunto y 256 MiB por lote.","Até 64 MiB por anexo e 256 MiB por lote.","Fino a 64 MiB per allegato e 256 MiB per invio.","До 64 МиБ на вложение и до 256 МиБ за раз."],
  memos: ['随记', 'Jots', '隨記', 'ひとことメモ', '한줄 메모', 'Notes rapides', 'Kurznotizen', 'Notas rápidas', 'Notas rápidas', 'Appunti veloci', 'Быстрые заметки'],
  memoPlaceholder: ['想记点什么？', 'What’s on your mind?', '想記點什麼？', '何をメモしますか？', '무엇을 기록할까요?', 'Que noter ?', 'Was möchtest du notieren?', '¿Qué quieres anotar?', 'O que quer anotar?', 'Cosa vuoi annotare?', 'Что хотите записать?'],
  memoPlaceholderTag: ['记录到 #{tag}…', 'Write a jot in #{tag}…', '記錄到 #{tag}…', '#{tag} にメモ…', '#{tag}에 기록…', 'Noter dans #{tag}…', 'In #{tag} notieren…', 'Anota en #{tag}…', 'Anote em #{tag}…', 'Annota in #{tag}…', 'Записать в #{tag}…'],
  memoEmpty: ['还没有随记', 'No jots yet', '還沒有隨記', 'メモはまだありません', '아직 메모가 없습니다', 'Aucune note rapide', 'Noch keine Kurznotizen', 'Aún no hay notas rápidas', 'Ainda não há notas rápidas', 'Nessun appunto veloce', 'Быстрых заметок пока нет'],
  memoEmptyHint: ['写下第一条', 'Write your first jot', '寫下第一條', '最初のメモを書こう', '첫 메모를 작성하세요', 'Écrivez votre première note', 'Schreib deine erste Kurznotiz', 'Escribe tu primera nota', 'Escreva sua primeira nota', 'Scrivi il tuo primo appunto', 'Напишите первую заметку'],
  memoTodo: ['待办', 'To-dos', '待辦', 'やること', '할 일', 'À faire', 'Aufgaben', 'Pendientes', 'Pendências', 'Da fare', 'Задачи'],
  memoEdited: ['已编辑', 'Edited', '已編輯', '編集済み', '수정됨', 'Modifié', 'Bearbeitet', 'Editado', 'Editado', 'Modificato', 'Изменено'],
  memoToNote: ['转为正式笔记…', 'Convert to Note…', '轉為正式筆記…', 'ノートに変換…', '노트로 변환…', 'Convertir en note…', 'In Notiz umwandeln…', 'Convertir en nota…', 'Converter em nota…', 'Converti in nota…', 'Преобразовать в заметку…'],
  memoCopy: ['复制文本', 'Copy Text', '複製文字', 'テキストをコピー', '텍스트 복사', 'Copier le texte', 'Text kopieren', 'Copiar texto', 'Copiar texto', 'Copia testo', 'Скопировать текст'],
  memoNoMatch: ['没有符合条件的随记', 'No matching jots', '沒有符合條件的隨記', '一致するメモはありません', '일치하는 메모가 없습니다', 'Aucune note correspondante', 'Keine passenden Kurznotizen', 'No hay notas que coincidan', 'Nenhuma nota correspondente', 'Nessun appunto corrispondente', 'Подходящих заметок нет'],
  memoClearFilter: ['清除筛选', 'Clear Filters', '清除篩選', 'フィルターを解除', '필터 지우기', 'Effacer les filtres', 'Filter zurücksetzen', 'Borrar filtros', 'Limpar filtros', 'Azzera filtri', 'Сбросить фильтры'],
  memoSave: ['记下', 'Save Jot', '記下', 'メモを保存', '메모 저장', 'Enregistrer', 'Kurznotiz speichern', 'Guardar nota', 'Salvar nota', 'Salva appunto', 'Сохранить заметку'],
  memoSearch: ['搜索随记', 'Search Jots', '搜尋隨記', 'メモを検索', '메모 검색', 'Rechercher des notes rapides', 'Kurznotizen suchen', 'Buscar notas rápidas', 'Buscar notas rápidas', 'Cerca appunti veloci', 'Поиск быстрых заметок'],
  memoPicture: ["插入图片","Insert Image","插入圖片","画像を挿入","이미지 삽입","Insérer une image","Bild einfügen","Insertar imagen","Inserir imagem","Inserisci immagine","Вставить изображение"],
  memoPlayAudio: ["播放音频","Play Audio","播放音訊","音声を再生","오디오 재생","Lire l’audio","Audio abspielen","Reproducir audio","Reproduzir áudio","Riproduci audio","Воспроизвести звук"],
  memoOpenAudio: ["打开音频文件","Open Audio File","開啟音訊檔案","音声ファイルを開く","오디오 파일 열기","Ouvrir le fichier audio","Audiodatei öffnen","Abrir archivo de audio","Abrir arquivo de áudio","Apri file audio","Открыть аудиофайл"],
  memoDock: ['写点什么…', 'Write something…', '寫點什麼…', '何か書く…', '무엇이든 적어 보세요…', 'Écrire quelque chose…', 'Etwas notieren…', 'Escribe algo…', 'Escreva algo…', 'Scrivi qualcosa…', 'Напишите что-нибудь…'],
  memoDraft: ['草稿：{text}', 'Draft: {text}', '草稿：{text}', '下書き：{text}', '초안: {text}', 'Brouillon : {text}', 'Entwurf: {text}', 'Borrador: {text}', 'Rascunho: {text}', 'Bozza: {text}', 'Черновик: {text}'],
  memoVoiceNote: ['语音速记', 'Voice Jot', '語音速記', '音声メモ', '음성 메모', 'Note vocale', 'Sprachnotiz', 'Nota de voz', 'Nota de voz', 'Nota vocale', 'Голосовая заметка'],
  memoRecordAudio: ["录制音频","Record Audio","錄製音訊","音声を録音","오디오 녹음","Enregistrer un audio","Audio aufnehmen","Grabar audio","Gravar áudio","Registra audio","Записать звук"],
  memoRecording: ["正在录音","Recording","正在錄音","録音中","녹음 중","Enregistrement","Aufnahme läuft","Grabando","Gravando","Registrazione in corso","Идёт запись"],
  memoStopRecording: ["停止录音","Stop Recording","停止錄音","録音を停止","녹음 중지","Arrêter l’enregistrement","Aufnahme stoppen","Detener grabación","Parar gravação","Interrompi registrazione","Остановить запись"],
  memoRecordingUnavailable: ["当前环境不支持录音","Audio recording is unavailable here.","目前的環境不支援錄音","この環境では録音できません","현재 환경에서는 녹음할 수 없습니다","L’enregistrement audio est indisponible ici.","Audioaufnahmen sind hier nicht verfügbar.","La grabación de audio no está disponible aquí.","A gravação de áudio não está disponível aqui.","La registrazione audio non è disponibile qui.","Запись звука здесь недоступна."],
  memoRecordingFailed: ["录音失败，请检查麦克风后重试","Recording failed. Check your microphone and try again.","錄音失敗，請檢查麥克風後重試","録音できませんでした。マイクを確認して再試行してください","녹음에 실패했습니다. 마이크를 확인한 후 다시 시도하세요","Échec de l’enregistrement. Vérifiez le microphone et réessayez.","Aufnahme fehlgeschlagen. Prüfe das Mikrofon und versuche es erneut.","Error al grabar. Comprueba el micrófono y vuelve a intentarlo.","Falha na gravação. Verifique o microfone e tente novamente.","Registrazione non riuscita. Controlla il microfono e riprova.","Не удалось записать звук. Проверьте микрофон и повторите попытку."],
  memoMicrophoneDenied: ["麦克风权限未开启，请允许访问后重试","Allow microphone access and try again.","麥克風權限未開啟，請允許存取後重試","マイクへのアクセスを許可して再試行してください","마이크 접근을 허용한 후 다시 시도하세요","Autorisez l’accès au microphone et réessayez.","Erlaube den Mikrofonzugriff und versuche es erneut.","Permite el acceso al micrófono y vuelve a intentarlo.","Permita o acesso ao microfone e tente novamente.","Consenti l’accesso al microfono e riprova.","Разрешите доступ к микрофону и повторите попытку."],
  memoLoadError: ['笔记库读取失败', 'Could not load the note library', '筆記庫讀取失敗', 'ノートライブラリを読み込めません', '노트 보관함을 불러올 수 없습니다', 'Impossible de charger la bibliothèque', 'Notizbibliothek konnte nicht geladen werden', 'No se pudo cargar la biblioteca', 'Não foi possível carregar a biblioteca', 'Impossibile caricare la raccolta', 'Не удалось загрузить библиотеку'],
  memoReturnToLibrary: ['请返回笔记库记录随记', 'Return to the library to save a jot', '請返回筆記庫記錄隨記', 'ライブラリに戻ってメモを書いてください', '보관함으로 돌아가 메모를 작성하세요', 'Revenez à la bibliothèque pour écrire', 'Zum Schreiben zur Bibliothek zurückkehren', 'Vuelve a la biblioteca para anotar', 'Volte à biblioteca para anotar', 'Torna alla raccolta per annotare', 'Вернитесь в библиотеку, чтобы записать'],
  memoTrashed: ['已移到废纸篓', 'Moved to Trash', '已移到垃圾桶', 'ゴミ箱に移動しました', '휴지통으로 이동했습니다', 'Déplacé vers la corbeille', 'In den Papierkorb verschoben', 'Enviado a la papelera', 'Movido para a lixeira', 'Spostato nel cestino', 'Перемещено в корзину'],
  memoConflict: ['冲突副本', 'Conflict Copy', '衝突副本', '競合コピー', '충돌 사본', 'Copie en conflit', 'Konfliktkopie', 'Copia en conflicto', 'Cópia em conflito', 'Copia in conflitto', 'Конфликтная копия'],
  memoReadOnly: ['只读', 'Read-only', '唯讀', '読み取り専用', '읽기 전용', 'Lecture seule', 'Schreibgeschützt', 'Solo lectura', 'Somente leitura', 'Sola lettura', 'Только чтение'],
  memoExpand: ['展开', 'Expand', '展開', '展開', '펼치기', 'Développer', 'Erweitern', 'Expandir', 'Expandir', 'Espandi', 'Развернуть'],
  memoCollapse: ['收起', 'Collapse', '收起', '折りたたむ', '접기', 'Réduire', 'Einklappen', 'Contraer', 'Recolher', 'Riduci', 'Свернуть'],
  memoAll: ['全部', 'All', '全部', 'すべて', '전체', 'Tout', 'Alle', 'Todo', 'Tudo', 'Tutti', 'Все'],
  memoSearchCount: ['{count} 条结果', '{count} results', '{count} 條結果', '{count} 件', '{count}개 결과', '{count} résultats', '{count} Ergebnisse', '{count} resultados', '{count} resultados', '{count} risultati', 'Найдено: {count}'],
  memoDropHint: ['松开以添加附件', 'Drop files to attach', '放開以加入附件', 'ドロップして添付', '놓아서 첨부하기', 'Déposez pour joindre', 'Zum Anhängen ablegen', 'Suelta para adjuntar', 'Solte para anexar', 'Rilascia per allegare', 'Отпустите, чтобы прикрепить'],
  memoDiscard: ['放弃对这条随记的修改？', 'Discard changes to this jot?', '放棄對這條隨記的修改？', 'このメモの変更を破棄しますか？', '이 메모의 변경 사항을 취소할까요?', 'Abandonner les modifications de cette note ?', 'Änderungen an dieser Kurznotiz verwerfen?', '¿Descartar los cambios de esta nota?', 'Descartar as alterações desta nota?', 'Annullare le modifiche a questo appunto?', 'Отменить изменения в этой заметке?'],
  memoDiscardAction: ['放弃修改', 'Discard Changes', '放棄修改', '変更を破棄', '변경 사항 취소', 'Abandonner', 'Änderungen verwerfen', 'Descartar cambios', 'Descartar alterações', 'Annulla modifiche', 'Отменить изменения'],
  memoChanged: ['这条随记已在别处修改，请重新编辑', 'This jot changed elsewhere. Open it again to edit.', '這條隨記已在別處修改，請重新編輯', 'メモが別の場所で変更されました。開き直してください。', '다른 곳에서 메모를 수정했습니다. 다시 열어 주세요.', 'Cette note a été modifiée ailleurs. Rouvrez-la.', 'Diese Kurznotiz wurde woanders geändert. Öffne sie erneut.', 'La nota cambió en otro lugar. Vuelve a abrirla.', 'A nota mudou em outro lugar. Abra-a novamente.', 'L’appunto è cambiato altrove. Riaprilo.', 'Заметка изменена в другом месте. Откройте её снова.'],
  memoAttachmentFailed: ["有 {count} 个附件未能添加","Could not add {count} attachment(s).","有 {count} 個附件未能加入","{count} 件の添付ファイルを追加できませんでした","첨부 파일 {count}개를 추가하지 못했습니다","Impossible d’ajouter {count} pièce(s) jointe(s).","{count} Anhänge konnten nicht hinzugefügt werden.","No se pudieron añadir {count} adjuntos.","Não foi possível adicionar {count} anexo(s).","Impossibile aggiungere {count} allegati.","Не удалось добавить вложения ({count})."],
  hideNoteSummaries: ['隐藏摘要', 'Hide Note Summaries', '隱藏摘要', 'ノートの要約を隠す', '노트 요약 숨기기', 'Masquer les résumés', 'Notizvorschauen ausblenden', 'Ocultar resúmenes', 'Ocultar resumos', 'Nascondi i riepiloghi', 'Скрыть краткое содержание'],
  notes: ['笔记', 'Notes', '筆記', 'ノート', '노트', 'Notes', 'Notizen', 'Notas', 'Notas', 'Note', 'Заметки'],
  allNotes: ['全部笔记', 'All Notes', '所有筆記', 'すべてのノート', '모든 노트', 'Toutes les notes', 'Alle Notizen', 'Todas las notas', 'Todas as notas', 'Tutte le note', 'Все заметки'],
  library: ['笔记库', 'Note Library', '筆記庫', 'ノートライブラリ', '노트 보관함', 'Bibliothèque de notes', 'Notizbibliothek', 'Biblioteca de notas', 'Biblioteca de notas', 'Libreria delle note', 'Библиотека заметок'],
  folders: ['文件夹', 'Folders', '資料夾', 'フォルダ', '폴더', 'Dossiers', 'Ordner', 'Carpetas', 'Pastas', 'Cartelle', 'Папки'],
  trash: ['废纸篓', 'Trash', '垃圾桶', 'ゴミ箱', '휴지통', 'Corbeille', 'Papierkorb', 'Papelera', 'Lixeira', 'Cestino', 'Корзина'],
  newNote: ['新建笔记', 'New Note', '新增筆記', '新規ノート', '새 노트', 'Nouvelle note', 'Neue Notiz', 'Nueva nota', 'Nova nota', 'Nuova nota', 'Новая заметка'],
  todayNote: ['今日笔记', 'Daily Note', '今日筆記', '今日のノート', '오늘의 노트', 'Note du jour', 'Tagesnotiz', 'Nota del día', 'Nota do dia', 'Nota del giorno', 'Заметка дня'],
  searchNotes: ['搜索笔记', 'Search Notes', '搜尋筆記', 'ノートを検索', '노트 검색', 'Rechercher des notes', 'Notizen suchen', 'Buscar notas', 'Buscar notas', 'Cerca note', 'Поиск заметок'],
  noNotes: ['此文件夹还没有笔记', 'No notes in this folder', '此資料夾尚無筆記', 'このフォルダにノートはありません', '이 폴더에 노트가 없습니다', 'Ce dossier ne contient aucune note', 'Keine Notizen in diesem Ordner', 'No hay notas en esta carpeta', 'Não há notas nesta pasta', 'Nessuna nota in questa cartella', 'В этой папке нет заметок'],
  noMatch: ['未找到匹配的笔记', 'No matching notes', '找不到符合的筆記', '一致するノートがありません', '일치하는 노트가 없습니다', 'Aucune note correspondante', 'Keine passenden Notizen', 'No hay notas que coincidan', 'Nenhuma nota correspondente', 'Nessuna nota corrispondente', 'Подходящих заметок нет'],
  emptyTrash: ['废纸篓为空', 'Trash is empty', '垃圾桶是空的', 'ゴミ箱は空です', '휴지통이 비어 있습니다', 'La corbeille est vide', 'Der Papierkorb ist leer', 'La papelera está vacía', 'A lixeira está vazia', 'Il cestino è vuoto', 'Корзина пуста'],
  noSelection: ['未选择笔记', 'No note selected', '未選取筆記', 'ノートが選択されていません', '선택한 노트가 없습니다', 'Aucune note sélectionnée', 'Keine Notiz ausgewählt', 'Ninguna nota seleccionada', 'Nenhuma nota selecionada', 'Nessuna nota selezionata', 'Заметка не выбрана'],
  missingNote: ['找不到这篇笔记', 'Note not found', '找不到這篇筆記', 'ノートが見つかりません', '노트를 찾을 수 없습니다', 'Note introuvable', 'Notiz nicht gefunden', 'No se encontró la nota', 'Nota não encontrada', 'Nota non trovata', 'Заметка не найдена'],
  share: ['分享', 'Share', '分享', '共有', '공유', 'Partager', 'Teilen', 'Compartir', 'Compartilhar', 'Condividi', 'Поделиться'],
  attach: ['添加附件', 'Add Attachment', '加入附件', '添付ファイルを追加', '첨부 파일 추가', 'Ajouter une pièce jointe', 'Anhang hinzufügen', 'Añadir adjunto', 'Adicionar anexo', 'Aggiungi allegato', 'Добавить вложение'],
  more: ['更多操作', 'More Actions', '更多操作', 'その他の操作', '추가 작업', 'Plus d’actions', 'Weitere Aktionen', 'Más acciones', 'Mais ações', 'Altre azioni', 'Другие действия'],
  editorToolbar: ['编辑器工具栏', 'Editor Toolbar', '編輯器工具列', 'エディターツールバー', '편집기 도구 모음', 'Barre d’outils de l’éditeur', 'Editor-Werkzeugleiste', 'Barra del editor', 'Barra do editor', 'Barra dell’editor', 'Панель редактора'],
  editor: ['所见即所得编辑区', 'Rich Text Editor', '所見即所得編輯區', 'リッチテキストエディター', '서식 있는 텍스트 편집기', 'Éditeur de texte enrichi', 'Rich-Text-Editor', 'Editor de texto enriquecido', 'Editor de texto rico', 'Editor di testo formattato', 'Редактор форматированного текста'],
  checklist: ['任务清单', 'Checklist', '待辦清單', 'チェックリスト', '체크리스트', 'Liste de tâches', 'Checkliste', 'Lista de tareas', 'Lista de tarefas', 'Elenco di attività', 'Список задач'],
  optimizeTypography: ['排版优化', 'Optimize Formatting', '排版優化', '体裁を整える', '서식 최적화', 'Optimiser la mise en forme', 'Formatierung optimieren', 'Optimizar formato', 'Otimizar formatação', 'Ottimizza formattazione', 'Оптимизировать оформление'],
  insertTable: ['插入表格', 'Insert Table', '插入表格', '表を挿入', '표 삽입', 'Insérer un tableau', 'Tabelle einfügen', 'Insertar tabla', 'Inserir tabela', 'Inserisci tabella', 'Вставить таблицу'],
  shortcuts: ['键盘快捷键', 'Keyboard Shortcuts', '鍵盤快速鍵', 'キーボードショートカット', '키보드 단축키', 'Raccourcis clavier', 'Tastaturkurzbefehle', 'Atajos de teclado', 'Atalhos de teclado', 'Scorciatoie da tastiera', 'Сочетания клавиш'],
  account: ['账户', 'Account', '帳號', 'アカウント', '계정', 'Compte', 'Konto', 'Cuenta', 'Conta', 'Account', 'Аккаунт'],
  profile: ['个人资料', 'Profile', '個人資料', 'プロフィール', '프로필', 'Profil', 'Profil', 'Perfil', 'Perfil', 'Profilo', 'Профиль'],
  personal: ['个人信息', 'Personal Information', '個人資訊', '個人情報', '개인 정보', 'Informations personnelles', 'Persönliche Angaben', 'Información personal', 'Informações pessoais', 'Dati personali', 'Личные данные'],
  signIn: ['登录与安全', 'Sign-in & Security', '登入與安全性', 'ログインとセキュリティ', '로그인 및 보안', 'Connexion et sécurité', 'Anmeldung und Sicherheit', 'Inicio de sesión y seguridad', 'Login e segurança', 'Accesso e sicurezza', 'Вход и безопасность'],
  localAccount: ['本地账户', 'Local Account', '本機帳號', 'ローカルアカウント', '로컬 계정', 'Compte local', 'Lokales Konto', 'Cuenta local', 'Conta local', 'Account locale', 'Локальный аккаунт'],
  storageSync: ['存储与同步', 'Storage & Sync', '儲存與同步', 'ストレージと同期', '저장소 및 동기화', 'Stockage et synchronisation', 'Speicher und Sync', 'Almacenamiento y sincronización', 'Armazenamento e sincronização', 'Archiviazione e sincronizzazione', 'Хранилище и синхронизация'],
  help: ['帮助与反馈', 'Help & Feedback', '說明與意見回饋', 'ヘルプとフィードバック', '도움말 및 의견 보내기', 'Aide et commentaires', 'Hilfe und Feedback', 'Ayuda y comentarios', 'Ajuda e feedback', 'Aiuto e feedback', 'Помощь и обратная связь'],
  about: ['关于轻羽', 'About QingYu', '關於輕羽', 'QingYu について', 'QingYu 정보', 'À propos de QingYu', 'Über QingYu', 'Acerca de QingYu', 'Sobre o QingYu', 'Informazioni su QingYu', 'О QingYu'],
  mcp: ['MCP 配置', 'MCP Configuration', 'MCP 設定', 'MCP 設定', 'MCP 구성', 'Configuration MCP', 'MCP-Konfiguration', 'Configuración de MCP', 'Configuração do MCP', 'Configurazione MCP', 'Настройка MCP'],
  done: ['完成', 'Done', '完成', '完了', '완료', 'Terminé', 'Fertig', 'Listo', 'Concluído', 'Fine', 'Готово'],
  closeSettings: ['关闭设置', 'Close Settings', '關閉設定', '設定を閉じる', '설정 닫기', 'Fermer les réglages', 'Einstellungen schließen', 'Cerrar ajustes', 'Fechar configurações', 'Chiudi impostazioni', 'Закрыть настройки'],
  backToSettings: ['返回设置', 'Back to Settings', '返回設定', '設定に戻る', '설정으로 돌아가기', 'Retour aux réglages', 'Zurück zu den Einstellungen', 'Volver a ajustes', 'Voltar às configurações', 'Torna alle impostazioni', 'Назад к настройкам'],
  back: ['后退', 'Back', '返回', '戻る', '뒤로', 'Précédent', 'Zurück', 'Atrás', 'Voltar', 'Indietro', 'Назад'],
  forward: ['前进', 'Forward', '前進', '進む', '앞으로', 'Suivant', 'Vorwärts', 'Adelante', 'Avançar', 'Avanti', 'Вперёд'],
  pin: ['置顶', 'Pin', '釘選', 'ピン留め', '고정', 'Épingler', 'Anheften', 'Fijar', 'Fixar', 'Fissa', 'Закрепить'],
  pinned: ['置顶', 'Pinned', '已釘選', 'ピン留め済み', '고정됨', 'Épinglées', 'Angeheftet', 'Fijadas', 'Fixadas', 'Fissate', 'Закреплённые'],
  unpin: ['取消置顶', 'Unpin', '取消釘選', 'ピン留めを解除', '고정 해제', 'Désépingler', 'Lösen', 'Desfijar', 'Desafixar', 'Rimuovi fissaggio', 'Открепить'],
  duplicate: ['复制笔记', 'Duplicate Note', '複製筆記', 'ノートを複製', '노트 복제', 'Dupliquer la note', 'Notiz duplizieren', 'Duplicar nota', 'Duplicar nota', 'Duplica nota', 'Дублировать заметку'],
  moveToTrash: ['移到废纸篓', 'Move to Trash', '移到垃圾桶', 'ゴミ箱に移動', '휴지통으로 이동', 'Mettre à la corbeille', 'In den Papierkorb', 'Mover a la papelera', 'Mover para a lixeira', 'Sposta nel cestino', 'Переместить в корзину'],
  restoreNote: ['恢复笔记', 'Restore Note', '還原筆記', 'ノートを復元', '노트 복원', 'Restaurer la note', 'Notiz wiederherstellen', 'Restaurar nota', 'Restaurar nota', 'Ripristina nota', 'Восстановить заметку'],
  openNoteWindow: ['在新窗口打开笔记', 'Open Note in New Window', '在新視窗打開筆記', '新しいウィンドウでノートを開く', '새 창에서 노트 열기', 'Ouvrir la note dans une nouvelle fenêtre', 'Notiz in neuem Fenster öffnen', 'Abrir nota en una ventana nueva', 'Abrir nota em nova janela', 'Apri nota in una nuova finestra', 'Открыть заметку в новом окне'],
  openNoteTab: ['在新标签页打开笔记', 'Open Note in New Tab', '在新分頁打開筆記', '新しいタブでノートを開く', '새 탭에서 노트 열기', 'Ouvrir la note dans un nouvel onglet', 'Notiz in neuem Tab öffnen', 'Abrir nota en una pestaña nueva', 'Abrir nota em nova aba', 'Apri nota in una nuova scheda', 'Открыть заметку в новой вкладке'],
  listOptions: ['列表选项', 'List Options', '列表選項', 'リストオプション', '목록 옵션', 'Options de la liste', 'Listenoptionen', 'Opciones de lista', 'Opções da lista', 'Opzioni elenco', 'Параметры списка'],
  searchSettings: ['搜索设置', 'Search Settings', '搜尋設定', '設定を検索', '설정 검색', 'Rechercher dans les réglages', 'Einstellungen durchsuchen', 'Buscar ajustes', 'Buscar configurações', 'Cerca impostazioni', 'Поиск настроек'],
  blankNote: ['这篇新笔记还是空的', 'This new note is empty', '這篇新筆記仍是空白的', 'この新規ノートは空です', '이 새 노트는 비어 있습니다', 'Cette nouvelle note est vide', 'Diese neue Notiz ist leer', 'Esta nota nueva está vacía', 'Esta nova nota está vazia', 'Questa nuova nota è vuota', 'Эта новая заметка пуста'],
  insertLink: ['插入链接', 'Insert Link', '插入連結', 'リンクを挿入', '링크 삽입', 'Insérer un lien', 'Link einfügen', 'Insertar enlace', 'Inserir link', 'Inserisci collegamento', 'Вставить ссылку'],
  noteTitleHint: ['标题', 'Title', '標題', 'タイトル', '제목', 'Titre', 'Titel', 'Título', 'Título', 'Titolo', 'Заголовок'],
  characterCount: ['{count} 字符', '{count} chars', '{count} 字元', '{count} 文字', '{count}자', '{count} car.', '{count} Zeichen', '{count} caract.', '{count} caract.', '{count} car.', '{count} симв.'],
  notesCount: ['{count} 篇笔记', 'Notes: {count}', '{count} 則筆記', '{count} 件のノート', '노트 {count}개', 'Notes : {count}', 'Notizen: {count}', 'Notas: {count}', 'Notas: {count}', 'Note: {count}', 'Заметок: {count}'],
  settingsCategories: ['设置分类', 'Settings Categories', '設定分類', '設定カテゴリ', '설정 카테고리', 'Catégories de réglages', 'Einstellungskategorien', 'Categorías de ajustes', 'Categorias de configurações', 'Categorie delle impostazioni', 'Категории настроек'],
  noSettings: ['没有匹配的设置', 'No matching settings', '找不到符合的設定', '一致する設定がありません', '일치하는 설정이 없습니다', 'Aucun réglage correspondant', 'Keine passenden Einstellungen', 'No hay ajustes que coincidan', 'Nenhuma configuração correspondente', 'Nessuna impostazione corrispondente', 'Подходящих настроек нет'],
  pastDays: ['过去 {count} 天', 'Last {count} Days', '過去 {count} 天', '過去 {count} 日間', '지난 {count}일', '{count} derniers jours', 'Letzte {count} Tage', 'Últimos {count} días', 'Últimos {count} dias', 'Ultimi {count} giorni', 'Последние {count} дней'],
  releaseNotes: ['更新内容', 'Release Notes', '更新內容', 'リリースノート', '릴리스 노트', 'Notes de version', 'Versionshinweise', 'Notas de la versión', 'Notas da versão', 'Note di rilascio', 'Примечания к выпуску'],
  sortCompleted: ['已完成的项目移到底部', 'Move completed items to the bottom', '將已完成的項目移到底部', '完了済みの項目を末尾に移動', '완료한 항목을 맨 아래로 이동', 'Placer les éléments terminés en bas', 'Erledigte Einträge nach unten verschieben', 'Mover las tareas completadas al final', 'Mover itens concluídos para o final', 'Sposta gli elementi completati in fondo', 'Перемещать выполненные пункты вниз'],
  toolbarVisibility: ['按钮显示', 'Toolbar Visibility', '按鈕顯示', 'ツールバーの表示', '도구 모음 표시', 'Affichage de la barre d’outils', 'Werkzeugleiste anzeigen', 'Visibilidad de la barra', 'Visibilidade da barra', 'Visibilità della barra', 'Видимость панели'],
  toolbarVisibilityHint: ['控制编辑区顶部工具按钮的显示方式', 'Choose when the editor toolbar is visible.', '選擇編輯器工具列的顯示時機。', 'エディターのツールバーを表示する条件を選びます。', '편집기 도구 모음을 표시할 시점을 선택합니다.', 'Choisissez quand afficher la barre d’outils de l’éditeur.', 'Wählen Sie, wann die Editor-Werkzeugleiste sichtbar ist.', 'Elige cuándo se muestra la barra del editor.', 'Escolha quando a barra do editor aparece.', 'Scegli quando mostrare la barra dell’editor.', 'Выберите, когда показывать панель редактора.'],
  alwaysShow: ['始终显示', 'Always', '一律顯示', '常に表示', '항상 표시', 'Toujours', 'Immer', 'Siempre', 'Sempre', 'Sempre', 'Всегда'],
  onHover: ['悬停时显示', 'On Hover', '滑鼠移入時顯示', 'ポインターを合わせたとき', '마우스를 올릴 때', 'Au survol', 'Beim Darüberfahren', 'Al pasar el cursor', 'Ao passar o cursor', 'Al passaggio del puntatore', 'При наведении'],
  noteWidth: ['正文宽度', 'Note Width', '正文寬度', 'ノート本文の幅', '노트 본문 너비', 'Largeur du texte', 'Textbreite', 'Ancho del texto', 'Largura do texto', 'Larghezza del testo', 'Ширина текста'],
  noteWidthHint: ['笔记编辑区的正文宽度', 'Width of the text in the note editor.', '筆記編輯區的正文寬度。', 'ノートエディターの本文の幅です。', '노트 편집기에서 본문의 너비입니다.', 'Largeur du texte dans l’éditeur de notes.', 'Breite des Textes im Notizeditor.', 'Ancho del texto en el editor de notas.', 'Largura do texto no editor de notas.', 'Larghezza del testo nell’editor delle note.', 'Ширина текста в редакторе заметок.'],
  fillWindow: ['铺满窗口', 'Fill Window', '填滿視窗', 'ウィンドウ幅に合わせる', '창 너비에 맞추기', 'Remplir la fenêtre', 'Fensterbreite nutzen', 'Ocupar toda la ventana', 'Preencher a janela', 'Riempi finestra', 'На всю ширину окна'],
  keepOnTop: ['窗口置顶', 'Keep Window on Top', '視窗置頂', 'ウィンドウを常に手前に表示', '창을 항상 위에 표시', 'Garder la fenêtre au premier plan', 'Fenster im Vordergrund halten', 'Mantener la ventana encima', 'Manter janela no topo', 'Tieni la finestra in primo piano', 'Держать окно поверх других'],
  keepOnTopHint: ['让轻羽窗口保持在其他窗口上方', 'Keep QingYu above other windows.', '讓輕羽視窗保持在其他視窗上方。', 'QingYu を他のウィンドウより手前に表示します。', 'QingYu 창을 다른 창 위에 유지합니다.', 'Gardez QingYu au-dessus des autres fenêtres.', 'QingYu über anderen Fenstern halten.', 'Mantén QingYu por encima de otras ventanas.', 'Mantenha o QingYu acima de outras janelas.', 'Mantieni QingYu sopra le altre finestre.', 'QingYu будет поверх остальных окон.'],
  systemLanguage: ['跟随系统', 'System Language', '跟隨系統', 'システム言語', '시스템 언어', 'Langue du système', 'Systemsprache', 'Idioma del sistema', 'Idioma do sistema', 'Lingua di sistema', 'Язык системы'],
} satisfies Record<string, MessageRow<typeof languages>>;

export type UiMessage = I18nKey | keyof typeof messages;

export function uiText(key: UiMessage, locale = globalThis.document?.documentElement.lang || 'zh-CN') {
  const language = isAppLanguage(locale) ? locale : 'zh-CN';
  return Object.hasOwn(messages, key) ? messages[key as keyof typeof messages][languages.indexOf(language)] : t(language, key);
}

export type WorkflowMessage = keyof typeof workflowCopy;

export function workflowText(source: WorkflowMessage, locale = globalThis.document?.documentElement.lang || 'zh-CN'): string {
  if (locale === 'zh-CN') return source;
  const translation = workflowCopy[source];
  if (typeof translation !== 'string') return uiText(translation[0], locale);
  if (locale === 'zh-TW') return traditionalWorkflowCopy[source] ?? source;
  return localizedWorkflowCopy[source]?.[workflowLocales.findIndex(code => code === locale)] ?? translation;
}

export function translatedLabel(key: UiMessage | WorkflowMessage) {
  return Object.hasOwn(workflowCopy, key) ? workflowText(key as WorkflowMessage) : uiText(key as UiMessage);
}

type LabelValues = Record<string, string | number>;

export function formatLabel(key: UiMessage | WorkflowMessage, values: LabelValues) {
  return translatedLabel(key).replace(/\{(\w+)\}/g, (token, name: string) =>
    Object.hasOwn(values, name) ? String(values[name]) : token);
}

export function markupLabel(key: UiMessage | WorkflowMessage, values?: LabelValues) {
  const text = values ? formatLabel(key, values) : translatedLabel(key);
  return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;').replaceAll("'", '&#39;');
}

export function localizeStatus(message: string) {
  const locale = globalThis.document?.documentElement.lang || 'zh-CN';
  const newline = message.indexOf('\n');
  const first = newline < 0 ? message : message.slice(0, newline);
  const details = newline < 0 ? '' : message.slice(newline);
  const read = /^Failed to read "([^"\n]+)" \(HTTP (\d+)\)(?:: (.*))?$/.exec(first);
  if (read) return formatLabel('无法读取「{path}」（HTTP {status}）', { path: read[1], status: read[2] })
    + (read[3] ? '\n' + read[3] : '') + details;
  if (locale === 'zh-CN') return message;
  if (Object.hasOwn(workflowCopy, first)) return workflowText(first as WorkflowMessage) + details;
  const summary = /^同步完成：(\d+) 篇笔记、(\d+) 个文件夹、(\d+) 个附件(，冲突已保留双方)?$/.exec(first);
  if (summary) {
    const count = (index: number) => Number(summary[index]).toLocaleString(locale);
    return formatLabel('同步完成：{notes} 篇笔记、{folders} 个文件夹、{assets} 个附件', { notes: count(1), folders: count(2), assets: count(3) })
      + (summary[4] ? workflowText('，冲突已保留双方') : '') + details;
  }
  const path = /^无法扫描同步文件：(.*)$/.exec(first);
  if (path) return formatLabel('无法扫描同步文件：{path}', { path: path[1] }) + details;
  const network = /^S3 网络请求失败：(.*)$/.exec(first);
  if (network) return formatLabel('S3 网络请求失败：{error}', { error: network[1] }) + details;
  const mcp = /^无法启动 MCP 服务：(.*)$/.exec(first);
  if (mcp) return formatLabel('无法启动 MCP 服务：{reason}', { reason: mcp[1] }) + details;
  const connectionTestDetails = () => details.replace(/^清理测试文件失败（HTTP (\d+)）$/gm, (_, status: string) =>
    formatLabel('{operation}失败（HTTP {status}）', { operation: workflowText('清理测试文件'), status }));
  const conditional = /^条件写入测试失败：预期 HTTP 412，实际 HTTP (\d+)$/.exec(first);
  if (conditional) return formatLabel('条件写入测试失败：预期 HTTP 412，实际 HTTP {status}', { status: conditional[1] }) + connectionTestDetails();
  const write = /^S3 条件写入失败（HTTP (\d+)）$/.exec(first);
  if (write) return formatLabel('S3 条件写入失败（HTTP {status}）', { status: write[1] }) + details;
  const operation = /^(写入测试|读取测试|条件写入测试|清理测试文件)失败（HTTP (\d+)）$/.exec(first);
  if (operation) return formatLabel('{operation}失败（HTTP {status}）', { operation: workflowText(operation[1] as WorkflowMessage), status: operation[2] }) + connectionTestDetails();
  return message;
}

export function localizeSyncError(message: string) {
  const translated = localizeStatus(message);
  const label = !message || translated !== message || !globalThis.document?.documentElement.lang
    || document.documentElement.lang === 'zh-CN'
    ? translated : formatLabel('同步失败：{message}', { message });
  return label.replaceAll('[已隐藏]', workflowText('[已隐藏]'));
}

// Only explicit UI labels are bound. Note text and user-supplied names never enter this lookup.
export function bindLabels(root: ParentNode) {
  const bindings = (['text', 'title', 'aria-label', 'placeholder'] as const).flatMap(attribute => {
    const marker = attribute === 'text' ? 'data-i18n' : `data-i18n-${attribute}`;
    return [...root.querySelectorAll(`[${marker}]`)].map(element => ({ element, attribute, marker }));
  });
  return () => {
    for (const { element, attribute, marker } of bindings) {
      const key = element.getAttribute(marker)!;
      const label = translatedLabel(key as UiMessage | WorkflowMessage) + (element.getAttribute(`${marker}-suffix`) ?? '');
      if (attribute === 'text') element.textContent = label;
      else element.setAttribute(attribute, label);
    }
  };
}
