(require 'cl-lib)
(require 'org)
(require 'org-agenda)

(let* ((case-dir (or (getenv "AGENDUM_CASE_DIR")
                     (error "AGENDUM_CASE_DIR is required")))
       (from (or (getenv "AGENDUM_FROM")
                 (error "AGENDUM_FROM is required")))
       (today (or (getenv "AGENDUM_TODAY")
                  (error "AGENDUM_TODAY is required")))
       (date-parts (mapcar #'string-to-number (split-string today "-")))
       (fixed-time (encode-time 0 0 12
                                (nth 2 date-parts)
                                (nth 1 date-parts)
                                (nth 0 date-parts)
                                t))
       (days (string-to-number (or (getenv "AGENDUM_DAYS") "1"))))
  (setq org-agenda-files (directory-files case-dir t "\\.org\\'")
        org-agenda-start-day from
        org-agenda-start-on-weekday nil
        org-agenda-span days
        org-agenda-use-time-grid nil
        org-agenda-include-diary nil
        org-agenda-skip-scheduled-if-done
        (equal (getenv "AGENDUM_SKIP_SCHEDULED_IF_DONE") "true")
        org-agenda-skip-deadline-if-done
        (equal (getenv "AGENDUM_SKIP_DEADLINE_IF_DONE") "true")
        org-agenda-skip-timestamp-if-done
        (equal (getenv "AGENDUM_SKIP_TIMESTAMP_IF_DONE") "true"))
  (cl-letf (((symbol-function 'current-time) (lambda () fixed-time)))
    (org-batch-agenda-csv "a")))
