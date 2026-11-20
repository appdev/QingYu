import { isAppLanguage, t, type AppLanguage, type I18nKey } from './vendor/markra/shared/i18n';
import { localizedWorkflowCopy, traditionalWorkflowCopy, workflowCopy, workflowLocales } from './workflow-copy';

const languages = ['zh-CN', 'en', 'zh-TW', 'ja', 'ko', 'fr', 'de', 'es', 'pt-BR', 'it', 'ru'] as const satisfies readonly AppLanguage[];
type MessageRow<Codes extends readonly string[]> = { readonly [Index in keyof Codes]: string };

// QingYu terms that have no equivalent in the Markra dictionaries.
const messages = {
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
