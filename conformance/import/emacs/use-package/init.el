(setq org-agenda-files nil)
(with-eval-after-load 'org
  (setq org-agenda-files
        (list (expand-file-name "tasks.org" (getenv "AGD_FIXTURE_DIR")))
        org-todo-keywords '((sequence "BACKLOG(b)" "DOING(g!)" "|" "DONE(d@)"))
        org-deadline-warning-days 9
        org-log-done 'note))
(require 'org)
