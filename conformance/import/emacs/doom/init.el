(defmacro after! (package &rest body)
  `(with-eval-after-load ',package ,@body))
(after! org
  (setq org-agenda-files
        (list (expand-file-name "tasks.org" (getenv "AGD_FIXTURE_DIR")))
        org-todo-keywords '((sequence "TODO(t)" "PROJ(p)" "|" "DONE(d!)"))
        org-agenda-span 'day
        org-agenda-start-on-weekday 1
        org-log-into-drawer t))
(require 'org)
